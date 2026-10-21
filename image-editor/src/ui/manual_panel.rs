//! Manual workbench: real pixel tools, docked layers and one undo step per gesture.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
#[path = "retouch_tools.rs"]
mod retouch_tools;
use super::{ImageEditor, ACCENT};
use crate::{Adjustment, BrushStyle, Document, Raster, Shape, Tool};
use egui::{Color32, Key, Pos2, Rect, Response, Sense, Stroke, TextureHandle, Ui, Vec2};

/// Global target for all manual pixel tools and clipboard operations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditTarget {
    /// Edit active-layer image pixels.
    #[default]
    Raster,
    /// Edit shared grayscale selection/inpaint coverage.
    Mask,
}

/// Manual workbench tool; AI operations remain in their separate dock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ManualTool {
    /// Translate active-layer pixels.
    Move,
    /// Replace the shared mask with a rectangular selection.
    Select,
    /// Freehand polygon selection using the shared grayscale mask.
    Lasso,
    /// Select a contiguous color region.
    Wand,
    /// Select matching colors across the canvas.
    ColorSelect,
    /// Copy pixels from an Alt-click source.
    Clone,
    /// Transfer texture with local color adaptation.
    Heal,
    /// Paint foreground color.
    #[default]
    Brush,
    /// Erase active-layer alpha.
    Eraser,
    /// Compatibility alias: select Mask target and the shared brush.
    Mask,
    /// Compatibility alias: select Mask target and the shared eraser.
    Unmask,
    /// Contiguous bucket fill.
    Fill,
    /// Foreground-to-background linear gradient.
    Gradient,
    /// Straight line.
    Line,
    /// Rectangle.
    Rectangle,
    /// Ellipse.
    Ellipse,
    /// Sample layer or composite color.
    Eyedropper,
    /// Define and confirm a crop rectangle.
    Crop,
    /// Drag the canvas viewport.
    Hand,
    /// Click to zoom in; secondary click zooms out.
    Zoom,
}
impl ManualTool {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Move => "Перемещение",
            Self::Select => "Прямоугольное выделение",
            Self::Lasso => "Лассо",
            Self::Wand => "Волшебная палочка",
            Self::ColorSelect => "Выбор по цвету",
            Self::Clone => "Штамп",
            Self::Heal => "Восстанавливающая кисть",
            Self::Brush => "Кисть",
            Self::Eraser => "Ластик",
            Self::Mask => "Кисть маски",
            Self::Unmask => "Ластик маски",
            Self::Fill => "Заливка",
            Self::Gradient => "Градиент",
            Self::Line => "Линия",
            Self::Rectangle => "Прямоугольник",
            Self::Ellipse => "Эллипс",
            Self::Eyedropper => "Пипетка",
            Self::Crop => "Кадрирование",
            Self::Hand => "Рука",
            Self::Zoom => "Масштаб",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Self::Move => "V",
            Self::Select => "M",
            Self::Brush => "B",
            Self::Eraser => "E",
            Self::Mask => "Q",
            Self::Unmask => "Shift+Q",
            Self::Fill => "G",
            Self::Gradient => "Shift+G",
            Self::Line => "Shift+L",
            Self::Lasso => "L",
            Self::Wand => "W",
            Self::ColorSelect => "Shift+W",
            Self::Clone => "S",
            Self::Heal => "J",
            Self::Rectangle => "U",
            Self::Ellipse => "Shift+U",
            Self::Eyedropper => "I",
            Self::Crop => "C",
            Self::Hand => "H",
            Self::Zoom => "Z",
        }
    }
    fn brush(self) -> Option<Tool> {
        match self {
            Self::Brush => Some(Tool::Brush),
            Self::Eraser => Some(Tool::Eraser),
            Self::Mask => Some(Tool::Mask),
            Self::Unmask => Some(Tool::MaskEraser),
            _ => None,
        }
    }
    fn edits_pixels(self) -> bool {
        !matches!(
            self,
            Self::Select
                | Self::Wand
                | Self::ColorSelect
                | Self::Lasso
                | Self::Mask
                | Self::Unmask
                | Self::Crop
                | Self::Eyedropper
                | Self::Hand
                | Self::Zoom
        )
    }
}

#[allow(clippy::struct_excessive_bools)] // Independent tool options and lifecycle flags.
pub(super) struct ManualUi {
    pub tool: ManualTool,
    pub target: EditTarget,
    retouch: retouch_tools::RetouchUi,
    other_colors: [Color32; 2],
    background: Color32,
    opacity: f32,
    hardness: f32,
    width: f32,
    filled: bool,
    respect_mask: bool,
    tolerance: u8,
    merged_sample: bool,
    lasso: Vec<[f32; 2]>,
    selection_op: Option<crate::SelectionOp>,
    start: Option<[f32; 2]>,
    last: Option<[f32; 2]>,
    stroke_before: Option<(Document, bool)>,
    property_before: Option<(Document, bool)>,
    pub thumbnails: Vec<TextureHandle>,
    rename: Option<(usize, String, bool)>,
    properties: bool,
    crop_pending: bool,
    ignore_until_release: bool,
    pub fit_once: bool,
}
impl Default for ManualUi {
    fn default() -> Self {
        Self {
            tool: ManualTool::Brush,
            target: EditTarget::Raster,
            retouch: retouch_tools::RetouchUi::default(),
            other_colors: [Color32::WHITE, Color32::BLACK],
            background: Color32::WHITE,
            opacity: 1.0,
            hardness: 1.0,
            width: 4.0,
            filled: false,
            respect_mask: false,
            tolerance: 0,
            merged_sample: true,
            lasso: Vec::new(),
            selection_op: None,
            start: None,
            last: None,
            stroke_before: None,
            property_before: None,
            thumbnails: Vec::new(),
            rename: None,
            properties: false,
            crop_pending: false,
            ignore_until_release: false,
            fit_once: true,
        }
    }
}
pub(super) enum LayerAction {
    Select(usize),
    New,
    Duplicate(usize),
    Remove(usize),
    Merge(usize),
    Visible(usize),
    Lock(usize),
    Reorder(usize, usize),
    Rename(usize, String),
    BeginRename(usize),
}

impl ImageEditor {
    /// Current shared target for painting, shapes, sampling and clipboard.
    pub fn edit_target(&self) -> EditTarget {
        self.manual_ui.target
    }
    /// Change target after committing the current gesture and floating paste.
    pub fn set_edit_target(&mut self, target: EditTarget) {
        if target == self.manual_ui.target {
            return;
        }
        let gesture = self.manual_ui.stroke_before.is_some()
            || self.manual_ui.start.is_some()
            || !self.manual_ui.lasso.is_empty();
        self.finish_manual_stroke();
        self.finish_pasted_object();
        self.layer_dialog = super::layer_dialog::LayerDialog::default();
        self.manual_ui.ignore_until_release |= gesture;
        self.manual_ui.start = None;
        self.manual_ui.last = None;
        self.manual_ui.lasso.clear();
        let old = [self.color, self.manual_ui.background];
        [self.color, self.manual_ui.background] = self.manual_ui.other_colors;
        self.manual_ui.other_colors = old;
        self.manual_ui.target = target;
        self.manual_ui.retouch = retouch_tools::RetouchUi::default();
        self.stop_segmentation_points();
        if target == EditTarget::Mask {
            self.generation_ui.show_mask = true;
        }
        self.changed = true;
    }
    pub(super) fn edit_manual(
        &mut self,
        operation: impl FnOnce(&mut Document) -> Result<(), crate::EditError>,
    ) {
        let mask = self.edit_target() == EditTarget::Mask;
        self.edit(|d| {
            if mask {
                d.edit_mask(operation)
            } else {
                operation(d)
            }
        });
    }
    /// Select a manual tool, leaving SAM point-placement mode.
    pub fn select_manual_tool(&mut self, tool: ManualTool) {
        if self.inline_transform() {
            self.layer_dialog = super::layer_dialog::LayerDialog::default();
            self.changed = true;
        }
        // Compatibility for embedding hosts; the palette exposes one brush/eraser.
        let tool = match tool {
            ManualTool::Mask => {
                self.set_edit_target(EditTarget::Mask);
                ManualTool::Brush
            }
            ManualTool::Unmask => {
                self.set_edit_target(EditTarget::Mask);
                ManualTool::Eraser
            }
            tool => tool,
        };
        if tool != ManualTool::Hand {
            self.finish_pasted_object();
        }
        let gesture = self.manual_ui.stroke_before.is_some()
            || self.manual_ui.start.is_some()
            || !self.manual_ui.lasso.is_empty();
        self.finish_manual_stroke();
        self.manual_ui.ignore_until_release |= gesture;
        self.manual_ui.start = None;
        self.manual_ui.last = None;
        self.manual_ui.lasso.clear();
        self.manual_ui.tool = tool;
        self.ribbon_ui.tab = if matches!(tool, ManualTool::Select | ManualTool::Lasso) {
            super::ribbon_ui::RibbonTab::Selection
        } else {
            super::ribbon_ui::RibbonTab::Tool
        };
        self.stop_segmentation_points();
    }
    fn brush_style(&self) -> BrushStyle {
        BrushStyle {
            opacity: self.manual_ui.opacity,
            hardness: self.manual_ui.hardness,
            respect_mask: self.edit_target() == EditTarget::Raster && self.manual_ui.respect_mask,
        }
    }
    pub(super) fn finish_manual_stroke(&mut self) {
        self.manual_ui.retouch.stroke = None;
        if let Some((before, was_unsaved)) = self.manual_ui.stroke_before.take() {
            if before == self.document {
                self.unsaved = was_unsaved;
            } else {
                self.history.checkpoint(&before);
                self.unsaved = true;
                self.manual_ui.thumbnails.clear();
                self.clear_segmentation_points();
            }
            self.manual_ui.last = None;
        }
    }
    fn finish_layer_property(&mut self) {
        if let Some((before, was_unsaved)) = self.manual_ui.property_before.take() {
            if before == self.document {
                self.unsaved = was_unsaved;
            } else {
                self.history.checkpoint(&before);
            }
        }
    }
    pub(super) fn restore_history(&mut self, redo: bool) {
        self.manual_ui.retouch = retouch_tools::RetouchUi::default();
        if self.layer_dialog_open() {
            self.changed = true;
        }
        self.layer_dialog = super::layer_dialog::LayerDialog::default();
        self.finish_pasted_object();
        self.manual_ui.ignore_until_release |= self.manual_ui.stroke_before.is_some()
            || self.manual_ui.start.is_some()
            || !self.manual_ui.lasso.is_empty();
        self.finish_manual_stroke();
        self.finish_layer_property();
        self.manual_ui.start = None;
        self.manual_ui.last = None;
        self.manual_ui.crop_pending = false;
        self.manual_ui.lasso.clear();
        self.manual_ui.selection_op = None;
        let changed = if redo {
            self.history.redo(&mut self.document)
        } else {
            self.history.undo(&mut self.document)
        };
        if changed {
            self.changed = true;
            self.unsaved = true;
            self.manual_ui.thumbnails.clear();
            self.clear_segmentation_points();
            let dims = [self.document.width(), self.document.height()];
            if self.dimensions != dims {
                self.dimensions = dims;
                self.crop = [0, 0, dims[0], dims[1]];
                self.manual_ui.fit_once = true;
            }
        }
    }
    pub(super) fn layer_action(&mut self, action: LayerAction) {
        self.manual_ui.retouch = retouch_tools::RetouchUi::default();
        if self.layer_dialog_open() {
            self.layer_dialog = super::layer_dialog::LayerDialog::default();
            self.changed = true;
        }
        self.finish_pasted_object();
        self.manual_ui.ignore_until_release |=
            self.manual_ui.stroke_before.is_some() || self.manual_ui.start.is_some();
        self.manual_ui.start = None;
        self.manual_ui.last = None;
        self.finish_manual_stroke();
        self.finish_layer_property();
        match action {
            LayerAction::Select(i) => {
                let _ = self.document.select(i);
            }
            LayerAction::BeginRename(i) => {
                self.manual_ui.rename = Some((i, self.document.layers()[i].name().into(), true));
            }
            LayerAction::New => self.edit(|d| {
                d.add_layer(
                    format!("Слой {}", d.layers().len() + 1),
                    Raster::solid(d.width(), d.height(), [0; 4])?,
                )
            }),
            LayerAction::Duplicate(i) => self.edit(|d| {
                d.select(i)?;
                d.duplicate_layer()
            }),
            LayerAction::Remove(i) => self.edit(|d| {
                d.select(i)?;
                d.remove_layer()
            }),
            LayerAction::Merge(i) => self.edit(|d| {
                d.select(i)?;
                d.merge_down()
            }),
            LayerAction::Reorder(from, to) => self.edit(|d| {
                d.reorder_layer(from, to)?;
                d.select(to)
            }),
            LayerAction::Visible(i) | LayerAction::Lock(i) | LayerAction::Rename(i, _) => {
                self.edit(|d| {
                    let active = d.active();
                    d.select(i)?;
                    let l = &d.layers()[i];
                    let (name, visible, opacity, locked) =
                        (l.name().to_owned(), l.visible(), l.opacity(), l.locked());
                    match action {
                        LayerAction::Visible(_) => d.set_layer(name, !visible, opacity),
                        LayerAction::Lock(_) => d.set_layer_locked(!locked),
                        LayerAction::Rename(_, name) => d.set_layer(name, visible, opacity),
                        _ => unreachable!(),
                    }
                    d.select(active)
                });
            }
        }
    }
    pub(super) fn clear_selection(&mut self) {
        let gesture = self.manual_ui.stroke_before.is_some()
            || self.manual_ui.start.is_some()
            || !self.manual_ui.lasso.is_empty();
        self.finish_manual_stroke();
        self.manual_ui.start = None;
        self.manual_ui.last = None;
        self.manual_ui.lasso.clear();
        self.manual_ui.ignore_until_release |= gesture;
        self.edit(|d| {
            d.clear_selection();
            Ok(())
        });
    }
    #[allow(clippy::too_many_lines)] // One keyboard dispatcher; tool variants share modifiers.
    pub(super) fn manual_shortcuts(&mut self, ui: &Ui) {
        if self.transform_preview_active() || ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        self.clipboard_shortcuts(ui);
        let mods = ui.input(|i| i.modifiers);
        if mods.command {
            if ui.input(|i| i.key_pressed(Key::T)) {
                self.start_canvas_transform();
            }
            if ui.input(|i| i.key_pressed(Key::D)) {
                self.clear_selection();
            }
            if ui.input(|i| i.key_pressed(Key::J)) {
                self.layer_action(LayerAction::Duplicate(self.document.active()));
            }
            if ui.input(|i| i.key_pressed(Key::E)) {
                self.layer_action(LayerAction::Merge(self.document.active()));
            }
            if mods.shift && ui.input(|i| i.key_pressed(Key::N)) {
                self.layer_action(LayerAction::New);
            }
            return;
        }
        if mods.alt {
            return;
        }
        if ui.input(|i| i.key_pressed(Key::Q)) {
            self.set_edit_target(if self.edit_target() == EditTarget::Raster {
                EditTarget::Mask
            } else {
                EditTarget::Raster
            });
        }
        for (key, tool) in [
            (Key::V, ManualTool::Move),
            (Key::M, ManualTool::Select),
            (
                Key::W,
                if mods.shift {
                    ManualTool::ColorSelect
                } else {
                    ManualTool::Wand
                },
            ),
            (Key::S, ManualTool::Clone),
            (Key::J, ManualTool::Heal),
            (Key::B, ManualTool::Brush),
            (Key::E, ManualTool::Eraser),
            (
                Key::G,
                if mods.shift {
                    ManualTool::Gradient
                } else {
                    ManualTool::Fill
                },
            ),
            (
                Key::L,
                if mods.shift {
                    ManualTool::Line
                } else {
                    ManualTool::Lasso
                },
            ),
            (
                Key::U,
                if mods.shift {
                    ManualTool::Ellipse
                } else {
                    ManualTool::Rectangle
                },
            ),
            (Key::I, ManualTool::Eyedropper),
            (Key::C, ManualTool::Crop),
            (Key::H, ManualTool::Hand),
            (Key::Z, ManualTool::Zoom),
        ] {
            if ui.input(|i| i.key_pressed(key)) {
                self.select_manual_tool(tool);
            }
        }
        if ui.input(|i| i.key_pressed(Key::X)) {
            std::mem::swap(&mut self.color, &mut self.manual_ui.background);
        }
        if ui.input(|i| i.key_pressed(Key::D)) {
            self.color = Color32::BLACK;
            self.manual_ui.background = Color32::WHITE;
        }
        if ui.input(|i| i.key_pressed(Key::OpenBracket)) {
            self.radius = (self.radius / 1.25).max(0.5);
        }
        if ui.input(|i| i.key_pressed(Key::CloseBracket)) {
            self.radius = (self.radius * 1.25).min(256.0);
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.manual_ui.retouch.stroke = None;
            if let Some((doc, unsaved)) = self.manual_ui.stroke_before.take() {
                self.document = doc;
                self.unsaved = unsaved;
                self.changed = true;
                self.manual_ui.thumbnails.clear();
            }
            self.manual_ui.start = None;
            self.manual_ui.last = None;
            self.manual_ui.crop_pending = false;
            self.manual_ui.lasso.clear();
            self.manual_ui.ignore_until_release = ui.input(|i| i.pointer.primary_down());
        }
        if self.manual_ui.crop_pending && ui.input(|i| i.key_pressed(Key::Enter)) {
            self.apply_manual_crop();
        }
    }
    #[allow(clippy::too_many_lines)] // Contextual ribbon groups share the selected tool state.
    pub(super) fn manual_options(&mut self, ui: &mut Ui) {
        use super::ribbon_ui::group;
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.horizontal_top(|ui| {
            let tool = self.manual_ui.tool;
            group(ui, "Инструмент", 175.0, |ui| {
                ui.strong(tool.label());
                ui.small(format!("Клавиша: {}", tool.key()));
                if tool == ManualTool::Move && ui.button("Трансформация · Ctrl+T").clicked()
                {
                    self.start_canvas_transform();
                }
            });
            if tool.brush().is_some() || matches!(tool, ManualTool::Clone | ManualTool::Heal) {
                group(ui, "Форма кисти", 215.0, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.radius)
                            .range(0.5..=256.0)
                            .prefix("Радиус ")
                            .suffix(" px"),
                    );
                    ui.add(
                        egui::Slider::new(&mut self.manual_ui.hardness, 0.0..=1.0)
                            .text("Жёсткость")
                            .show_value(false),
                    );
                });
            }
            if (tool.edits_pixels() && tool != ManualTool::Move) || tool.brush().is_some() {
                group(ui, "Наложение", 235.0, |ui| {
                    ui.add(
                        egui::Slider::new(&mut self.manual_ui.opacity, 0.0..=1.0)
                            .text("Непрозрачность")
                            .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                            .custom_parser(|v| {
                                v.trim_end_matches('%')
                                    .parse::<f64>()
                                    .ok()
                                    .map(|v| v / 100.0)
                            }),
                    );
                    if self.edit_target() == EditTarget::Raster {
                        ui.checkbox(&mut self.manual_ui.respect_mask, "Внутри маски ИИ");
                    }
                });
            }
            match tool {
                ManualTool::Wand | ManualTool::ColorSelect => {
                    group(ui, "Образец", 210.0, |ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.manual_ui.tolerance)
                                .range(0..=255)
                                .prefix("Допуск "),
                        );
                        if self.edit_target() == EditTarget::Raster {
                            ui.checkbox(&mut self.manual_ui.merged_sample, "Все видимые слои");
                        } else {
                            ui.label("Покрытие маски: 0..255");
                        }
                        ui.small(if tool == ManualTool::Wand {
                            "Только соседние пиксели"
                        } else {
                            "По всему холсту"
                        });
                    });
                    group(ui, "Совмещение выделений", 225.0, |ui| {
                        ui.horizontal(|ui| {
                            ui.selectable_value(
                                &mut self.ribbon_ui.selection_op,
                                crate::SelectionOp::Replace,
                                "Заменить",
                            );
                            ui.selectable_value(
                                &mut self.ribbon_ui.selection_op,
                                crate::SelectionOp::Add,
                                "Добавить",
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.selectable_value(
                                &mut self.ribbon_ui.selection_op,
                                crate::SelectionOp::Subtract,
                                "Вычесть",
                            );
                            ui.selectable_value(
                                &mut self.ribbon_ui.selection_op,
                                crate::SelectionOp::Intersect,
                                "Пересечь",
                            );
                        });
                        ui.small("Shift: добавить · Alt: вычесть");
                    });
                }
                ManualTool::Clone | ManualTool::Heal => {
                    group(ui, "Источник ретуши", 310.0, |ui| {
                        ui.small(if self.edit_target() == EditTarget::Mask {
                            "Alt+ЛКМ — задать источник в маске"
                        } else {
                            "Alt+ЛКМ — источник на активном слое"
                        });
                        ui.small("Каждый штрих начинается с выбранного источника");
                        if self.manual_ui.retouch.source.is_some() {
                            if ui.button("Сбросить источник").clicked() {
                                self.manual_ui.retouch = retouch_tools::RetouchUi::default();
                            }
                        } else {
                            ui.weak("Источник ещё не задан");
                        }
                    });
                }
                ManualTool::Line | ManualTool::Rectangle | ManualTool::Ellipse => {
                    group(ui, "Фигура", 190.0, |ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.manual_ui.width)
                                .range(1.0..=256.0)
                                .prefix("Линия ")
                                .suffix(" px"),
                        );
                        if tool != ManualTool::Line {
                            ui.checkbox(&mut self.manual_ui.filled, "Заливка");
                        }
                    });
                }
                ManualTool::Fill => group(ui, "Заливка", 160.0, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.manual_ui.tolerance)
                            .range(0..=255)
                            .prefix("Допуск "),
                    );
                }),
                ManualTool::Eyedropper => group(ui, "Источник цвета", 200.0, |ui| {
                    if self.edit_target() == EditTarget::Raster {
                        ui.checkbox(&mut self.manual_ui.merged_sample, "Все видимые слои");
                    } else {
                        ui.label("Покрытие маски: 0..255");
                    }
                }),
                ManualTool::Select | ManualTool::Lasso => {
                    group(ui, "Выделение", 220.0, |ui| {
                        if ui.button("Снять · Ctrl+D").clicked() {
                            self.clear_selection();
                        }
                        if ui.button("Параметры выделения").clicked() {
                            self.ribbon_ui.tab = super::ribbon_ui::RibbonTab::Selection;
                        }
                    });
                }
                ManualTool::Crop => {
                    group(ui, "Кадрирование холста", 300.0, |ui| {
                        ui.small("Выделите область · Enter — применить · Esc — отменить");
                        if ui
                            .add_enabled(
                                self.manual_ui.crop_pending,
                                egui::Button::new("Кадрировать"),
                            )
                            .clicked()
                        {
                            self.apply_manual_crop();
                        }
                    });
                }
                ManualTool::Move => group(ui, "Перемещение", 225.0, |ui| {
                    ui.label(if self.edit_target() == EditTarget::Mask {
                        "Перетащите маску"
                    } else {
                        "Перетащите активный слой"
                    });
                }),
                ManualTool::Hand => group(ui, "Навигация / вставка", 300.0, |ui| {
                    ui.small("ЛКМ — фрагмент после вставки или холст · ПКМ — холст");
                    if self.has_pasted_object() && ui.button("Закрепить вставку · Enter").clicked()
                    {
                        self.finish_pasted_object();
                    }
                }),
                ManualTool::Zoom => group(ui, "Масштаб", 230.0, |ui| {
                    ui.label("ЛКМ + / ПКМ − · Ctrl+колесо");
                    if ui.button("По размеру").clicked() {
                        self.manual_ui.fit_once = true;
                    }
                }),
                _ => {}
            }
        });
    }
    #[allow(clippy::too_many_lines)] // Compact palette slots, popup variants and color footer.
    pub(super) fn manual_palette(&mut self, ui: &mut Ui) {
        let slots: &[&[ManualTool]] = &[
            &[ManualTool::Move],
            &[
                ManualTool::Select,
                ManualTool::Lasso,
                ManualTool::Wand,
                ManualTool::ColorSelect,
            ],
            &[ManualTool::Brush],
            &[ManualTool::Eraser],
            &[ManualTool::Clone, ManualTool::Heal],
            &[ManualTool::Fill, ManualTool::Gradient],
            &[ManualTool::Line, ManualTool::Rectangle, ManualTool::Ellipse],
            &[ManualTool::Eyedropper],
            &[ManualTool::Crop],
            &[ManualTool::Hand],
            &[ManualTool::Zoom],
        ];
        egui::ScrollArea::vertical()
            .id_salt("tool-strip-scroll")
            .max_height((ui.available_height() - 46.0).max(64.0))
            .show(ui, |ui| {
                for (i, variants) in slots.iter().enumerate() {
                    if [2, 4, 6, 8].contains(&i) {
                        ui.add_space(3.0);
                        ui.separator();
                    }
                    let tool = variants
                        .iter()
                        .copied()
                        .find(|t| *t == self.manual_ui.tool)
                        .unwrap_or(variants[0]);
                    let active = self.manual_ui.tool == tool && !self.segmentation_point_mode();
                    let response = ui
                        .add_sized([36.0, 34.0], egui::Button::new("").selected(active))
                        .on_hover_text(format!(
                            "{} · {}{}",
                            tool.label(),
                            tool.key(),
                            if variants.len() > 1 {
                                " · ПКМ: варианты"
                            } else {
                                ""
                            }
                        ));
                    icon(
                        ui.painter(),
                        response.rect.shrink(7.0),
                        tool,
                        if active {
                            Color32::WHITE
                        } else {
                            Color32::from_gray(202)
                        },
                    );
                    if variants.len() > 1 {
                        let r = response.rect;
                        ui.painter().add(egui::Shape::convex_polygon(
                            vec![
                                r.right_bottom() - Vec2::new(3.0, 3.0),
                                r.right_bottom() - Vec2::new(9.0, 3.0),
                                r.right_bottom() - Vec2::new(3.0, 9.0),
                            ],
                            Color32::from_gray(180),
                            Stroke::NONE,
                        ));
                        response.context_menu(|ui| {
                            for &variant in *variants {
                                if ui
                                    .button(format!("{} · {}", variant.label(), variant.key()))
                                    .clicked()
                                {
                                    self.select_manual_tool(variant);
                                    ui.close();
                                }
                            }
                        });
                    }
                    if response.clicked() {
                        self.select_manual_tool(tool);
                    }
                }
            });
        let response = ui
            .menu_button("   ", |ui| {
                ui.set_width(245.0);
                self.manual_colors(ui);
            })
            .response;
        let back = response.rect.shrink(5.0).translate(Vec2::new(3.0, 3.0));
        ui.painter()
            .rect_filled(back, 1.0, self.manual_ui.background);
        let front = back.translate(Vec2::new(-5.0, -5.0));
        ui.painter().rect_filled(front, 1.0, self.color);
        ui.painter().rect_stroke(
            front,
            1.0,
            Stroke::new(1.0, Color32::GRAY),
            egui::StrokeKind::Inside,
        );
        response.on_hover_text("Цвет и фон · X — поменять · D — чёрный/белый");
    }
    pub(super) fn manual_dock(&mut self, ui: &mut Ui, _actions: &mut Vec<super::FileAction>) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.manual_ui.properties, false, "Слои");
            ui.selectable_value(&mut self.manual_ui.properties, true, "Свойства");
        });
        ui.separator();
        if self.manual_ui.properties {
            egui::ScrollArea::vertical()
                .id_salt("manual-properties")
                .show(ui, |ui| self.manual_properties(ui));
        } else {
            self.layers_dock(ui);
        }
        self.rename_dialog(ui);
    }
    pub(super) fn manual_colors(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Цвет");
            if self.edit_target() == EditTarget::Mask {
                let mut value = self.color.r();
                ui.add(egui::DragValue::new(&mut value).range(0..=255));
                self.color = Color32::from_gray(value);
            } else {
                ui.color_edit_button_srgba(&mut self.color);
            }
            ui.label("Фон");
            if self.edit_target() == EditTarget::Mask {
                let mut value = self.manual_ui.background.r();
                ui.add(egui::DragValue::new(&mut value).range(0..=255));
                self.manual_ui.background = Color32::from_gray(value);
            } else {
                ui.color_edit_button_srgba(&mut self.manual_ui.background);
            }
            let swap = ui
                .add_sized([32.0, 32.0], egui::Button::new(""))
                .on_hover_text("Поменять цвета · X");
            action_icon(
                ui.painter(),
                swap.rect.shrink(9.0),
                6,
                Color32::from_gray(220),
            );
            if swap.clicked() {
                std::mem::swap(&mut self.color, &mut self.manual_ui.background);
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().interact_size = Vec2::splat(18.0);
            ui.spacing_mut().button_padding = Vec2::ZERO;
            ui.spacing_mut().item_spacing = Vec2::splat(5.0);
            for mut color in [
                Color32::BLACK,
                Color32::WHITE,
                Color32::from_gray(128),
                ACCENT,
                Color32::from_rgb(80, 160, 240),
                Color32::from_rgb(219, 66, 68),
                Color32::from_rgb(232, 139, 65),
                Color32::from_rgb(242, 204, 87),
                Color32::from_rgb(76, 164, 116),
                Color32::from_rgb(78, 183, 184),
                Color32::from_rgb(146, 106, 202),
                Color32::from_rgb(203, 116, 168),
            ] {
                if self.edit_target() == EditTarget::Mask {
                    let value = ((54 * u32::from(color.r())
                        + 183 * u32::from(color.g())
                        + 19 * u32::from(color.b())
                        + 128)
                        / 256) as u8;
                    color = Color32::from_gray(value);
                }
                let r = ui
                    .add_sized([18.0, 18.0], egui::Button::new("").fill(color))
                    .on_hover_text(format!(
                        "#{:02X}{:02X}{:02X}",
                        color.r(),
                        color.g(),
                        color.b()
                    ));
                if r.clicked() {
                    self.color = color;
                }
            }
        });
    }
    pub(super) fn manual_properties_open(&mut self) {
        self.manual_ui.properties = true;
    }
    #[allow(clippy::too_many_lines)] // Declarative rows, drop targets and action bar share one layout.
    fn layers_dock(&mut self, ui: &mut Ui) {
        let active = self.document.active();
        let layer = &self.document.layers()[active];
        let (name, visible, opacity) = (layer.name().to_owned(), layer.visible(), layer.opacity());
        ui.horizontal(|ui| {
            ui.strong("Слои");
            ui.weak(format!("{} / 16", self.document.layers().len()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak("Обычный");
            });
        });
        let mut percent = f32::from(opacity) * 100.0 / 255.0;
        let response = ui.add(
            egui::Slider::new(&mut percent, 0.0..=100.0)
                .text("Непрозрачность")
                .suffix("%"),
        );
        if response.drag_started() {
            self.manual_ui.property_before = Some((self.document.clone(), self.unsaved));
        }
        if response.changed() {
            let value = (percent * 255.0 / 100.0).round() as u8;
            if self.manual_ui.property_before.is_some() {
                self.document.set_layer(name, visible, value);
                self.changed = true;
                self.unsaved = true;
            } else {
                let thumbnails = std::mem::take(&mut self.manual_ui.thumbnails);
                self.edit(|d| {
                    d.set_layer(name, visible, value);
                    Ok(())
                });
                self.manual_ui.thumbnails = thumbnails;
            }
        }
        if response.drag_stopped() {
            self.finish_layer_property();
        }
        if self.manual_ui.thumbnails.len() != self.document.layers().len() {
            self.manual_ui.thumbnails = self
                .document
                .layers()
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    let raster = l.raster().thumbnail(48).expect("bounded thumbnail");
                    ui.ctx().load_texture(
                        format!("layer-thumbnail-{i}"),
                        egui::ColorImage::from_rgba_unmultiplied(
                            [raster.width() as usize, raster.height() as usize],
                            raster.pixels(),
                        ),
                        egui::TextureOptions::LINEAR,
                    )
                })
                .collect();
        }
        let mut action = None;
        let height = (ui.available_height() - 84.0).max(60.0);
        egui::ScrollArea::vertical()
            .id_salt("layer-stack")
            .max_height(height)
            .min_scrolled_height(height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for i in (0..self.document.layers().len()).rev() {
                    let layer = &self.document.layers()[i];
                    let name = layer.name().to_owned();
                    let visible = layer.visible();
                    let locked = layer.locked();
                    let opacity = layer.opacity();
                    let texture = &self.manual_ui.thumbnails[i];
                    let row = ui.scope(|ui| {
                        let (rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 58.0),
                            Sense::hover(),
                        );
                        // Register the row before its child buttons so eye/lock retain input.
                        let response = ui
                            .interact(
                                rect,
                                egui::Id::new(("layer-row", i)),
                                Sense::click_and_drag(),
                            )
                            .on_hover_cursor(egui::CursorIcon::Grab);
                        let fill = if active == i {
                            Color32::from_rgb(42, 61, 87)
                        } else if response.hovered() {
                            Color32::from_gray(62)
                        } else {
                            Color32::from_rgb(42, 42, 46)
                        };
                        ui.painter().rect_filled(rect, 4.0, fill);
                        if active == i {
                            ui.painter().rect_filled(
                                Rect::from_min_size(rect.min, Vec2::new(3.0, 58.0)),
                                2.0,
                                ACCENT,
                            );
                        }
                        let eye =
                            Rect::from_min_size(rect.min + Vec2::new(5.0, 17.0), Vec2::splat(24.0));
                        let er = ui
                            .put(eye, egui::Button::new("").frame(false))
                            .on_hover_text("Видимость слоя");
                        eye_icon(ui.painter(), eye.shrink(5.0), visible);
                        if er.clicked() {
                            action = Some(LayerAction::Visible(i));
                        }
                        let thumb =
                            Rect::from_min_size(rect.min + Vec2::new(34.0, 7.0), Vec2::splat(44.0));
                        checker(ui.painter(), thumb, 6.0);
                        let size = texture.size_vec2();
                        let size = size * (40.0 / size.x.max(size.y));
                        ui.painter().image(
                            texture.id(),
                            Rect::from_center_size(thumb.center(), size),
                            Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                        let title = Rect::from_min_max(
                            rect.min + Vec2::new(86.0, 8.0),
                            egui::pos2(rect.right() - 31.0, rect.top() + 30.0),
                        );
                        ui.put(
                            title,
                            egui::Label::new(egui::RichText::new(&name).strong())
                                .truncate()
                                .selectable(false)
                                .sense(Sense::hover()),
                        );
                        ui.painter().text(
                            rect.min + Vec2::new(86.0, 34.0),
                            egui::Align2::LEFT_TOP,
                            format!(
                                "{}% · {}×{}",
                                u32::from(opacity) * 100 / 255,
                                self.document.width(),
                                self.document.height()
                            ),
                            egui::FontId::proportional(10.0),
                            Color32::from_gray(153),
                        );
                        let lock = Rect::from_min_size(
                            egui::pos2(rect.right() - 29.0, rect.top() + 17.0),
                            Vec2::splat(24.0),
                        );
                        if ui
                            .put(lock, egui::Button::new("").frame(false))
                            .on_hover_text("Блокировка пикселей")
                            .clicked()
                        {
                            action = Some(LayerAction::Lock(i));
                        }
                        lock_icon(ui.painter(), lock.shrink(5.0), locked);
                        response
                    });
                    let response = row.inner;
                    if response.dragged() {
                        egui::DragAndDrop::set_payload(ui.ctx(), i);
                    }
                    if response.clicked() && action.is_none() {
                        action = Some(LayerAction::Select(i));
                    }
                    if response.double_clicked() && action.is_none() {
                        action = Some(LayerAction::BeginRename(i));
                    }
                    if response.dnd_hover_payload::<usize>().is_some() {
                        let above = ui
                            .input(|input| input.pointer.hover_pos())
                            .is_some_and(|p| p.y < response.rect.center().y);
                        let y = if above {
                            response.rect.top()
                        } else {
                            response.rect.bottom()
                        };
                        ui.painter().line_segment(
                            [
                                egui::pos2(response.rect.left(), y),
                                egui::pos2(response.rect.right(), y),
                            ],
                            Stroke::new(2.0, ACCENT),
                        );
                    }
                    if let Some(from) = response.dnd_release_payload::<usize>() {
                        if *from != i {
                            let above = ui
                                .input(|input| input.pointer.hover_pos())
                                .is_some_and(|p| p.y < response.rect.center().y);
                            let to = i - usize::from(*from < i) + usize::from(above);
                            action = Some(LayerAction::Reorder(
                                *from,
                                to.min(self.document.layers().len() - 1),
                            ));
                        }
                    }
                    response.context_menu(|ui| {
                        if ui.button("Переименовать…").clicked() {
                            action = Some(LayerAction::BeginRename(i));
                            ui.close();
                        }
                        if ui.button("Дублировать · Ctrl+J").clicked() {
                            action = Some(LayerAction::Duplicate(i));
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                self.can_merge(i),
                                egui::Button::new("Объединить с нижним · Ctrl+E"),
                            )
                            .clicked()
                        {
                            action = Some(LayerAction::Merge(i));
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                !locked && self.document.layers().len() > 1,
                                egui::Button::new("Удалить слой"),
                            )
                            .clicked()
                        {
                            action = Some(LayerAction::Remove(i));
                            ui.close();
                        }
                    });
                    ui.add_space(3.0);
                }
            });
        ui.separator();
        ui.horizontal(|ui| {
            for (index, (enabled, _label, tip, a)) in [
                (
                    self.document.layers().len() < 16,
                    "+",
                    "Новый прозрачный слой · Ctrl+Shift+N",
                    LayerAction::New,
                ),
                (
                    self.document.layers().len() < 16,
                    "⧉",
                    "Дублировать слой · Ctrl+J",
                    LayerAction::Duplicate(active),
                ),
                (
                    self.can_merge(active),
                    "⇩",
                    "Объединить с нижним · Ctrl+E",
                    LayerAction::Merge(active),
                ),
                (
                    active + 1 < self.document.layers().len(),
                    "↑",
                    "Выше",
                    LayerAction::Reorder(
                        active,
                        (active + 1).min(self.document.layers().len() - 1),
                    ),
                ),
                (
                    active > 0,
                    "↓",
                    "Ниже",
                    LayerAction::Reorder(active, active.saturating_sub(1)),
                ),
                (
                    !self.document.layers()[active].locked() && self.document.layers().len() > 1,
                    "×",
                    "Удалить слой",
                    LayerAction::Remove(active),
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let response = ui
                    .add_enabled(enabled, egui::Button::new("").min_size(Vec2::splat(28.0)))
                    .on_hover_text(tip);
                action_icon(
                    ui.painter(),
                    response.rect.shrink(8.0),
                    index,
                    if enabled {
                        Color32::from_gray(220)
                    } else {
                        Color32::from_gray(100)
                    },
                );
                if response.clicked() {
                    action = Some(a);
                }
            }
        });
        ui.small("Двойной щелчок — имя · перетащите для порядка");
        if let Some(action) = action {
            self.layer_action(action);
        }
    }
    fn lasso_canvas(
        &mut self,
        ui: &Ui,
        response: &Response,
        point: Option<[f32; 2]>,
        engaged: bool,
        down: bool,
    ) {
        if engaged && self.manual_ui.lasso.is_empty() {
            self.manual_ui.selection_op = Some(self.selection_operation(ui));
        }
        if engaged || (!down && !self.manual_ui.lasso.is_empty()) {
            if let Some(point) = point {
                let path = &mut self.manual_ui.lasso;
                if path.last().is_none_or(|last| {
                    (point[0] - last[0]).powi(2) + (point[1] - last[1]).powi(2) >= 0.25
                }) {
                    if path.len() < 4096 {
                        path.push(point);
                    } else if let Some(last) = path.last_mut() {
                        *last = point;
                    }
                }
            }
        }
        let path: Vec<_> = self
            .manual_ui
            .lasso
            .iter()
            .map(|p| response.rect.min + Vec2::new(p[0], p[1]) * self.zoom)
            .collect();
        let painter = ui
            .painter()
            .with_clip_rect(response.rect.intersect(ui.clip_rect()));
        if path.len() > 1 {
            painter.add(egui::Shape::line(
                path.clone(),
                Stroke::new(1.5, Color32::WHITE),
            ));
            painter.line_segment(
                [path[0], *path.last().expect("path")],
                Stroke::new(1.0, Color32::GRAY),
            );
        }
        if !down && !self.manual_ui.lasso.is_empty() {
            let points = std::mem::take(&mut self.manual_ui.lasso);
            if points.len() >= 3 {
                let op = self
                    .manual_ui
                    .selection_op
                    .take()
                    .unwrap_or_else(|| self.selection_operation(ui));
                self.edit(|d| d.select_area_polygon(&points, op));
            }
        }
    }
    fn can_merge(&self, i: usize) -> bool {
        i > 0
            && self.document.layers()[i].visible()
            && !self.document.layers()[i].locked()
            && self.document.layers()[i - 1].visible()
            && !self.document.layers()[i - 1].locked()
    }
    fn rename_dialog(&mut self, ui: &Ui) {
        let (mut save, mut cancel) = (false, false);
        if let Some((_, name, focus)) = &mut self.manual_ui.rename {
            egui::Window::new("Имя слоя")
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .default_width(300.0)
                .show(ui.ctx(), |ui| {
                    let response = ui.text_edit_singleline(name);
                    if *focus {
                        response.request_focus();
                        *focus = false;
                    }
                    save =
                        ui.button("Сохранить").clicked() || ui.input(|i| i.key_pressed(Key::Enter));
                    cancel =
                        ui.button("Отмена").clicked() || ui.input(|i| i.key_pressed(Key::Escape));
                });
        }
        if cancel {
            self.manual_ui.rename = None;
        } else if save {
            if let Some((i, name, _)) = self.manual_ui.rename.take() {
                self.layer_action(LayerAction::Rename(i, name));
            }
        }
    }
    fn manual_properties(&mut self, ui: &mut Ui) {
        ui.strong("Холст");
        ui.label(format!(
            "{} × {} px",
            self.document.width(),
            self.document.height()
        ));
        ui.horizontal_wrapped(|ui| {
            if ui.button("Новый…").clicked() {
                self.confirm_new = true;
            }
            if ui.button("Размер…").clicked() {
                self.open_canvas_dialog();
            }
            if ui.button("Поворот…").clicked() {
                self.open_rotation_dialog();
            }
        });
        if ui.button("Кадрирование · C").clicked() {
            self.select_manual_tool(ManualTool::Crop);
        }
        ui.small("Выделите область на холсте · Enter — обрезать · Esc — отменить");
        if ui
            .button("Очистить холст")
            .on_hover_text(
                "Удалить все слои и маску, создать новый пустой слой. Ctrl+Z — отменить.",
            )
            .clicked()
        {
            self.edit(Document::clear_canvas);
        }
        if ui
            .button("Удалить остальные слои")
            .on_hover_text("Оставить текущий слой с его изображением")
            .clicked()
        {
            self.edit(Document::retain_active_layer);
        }
        ui.checkbox(&mut self.pixel_grid, "Пиксельная сетка");
        ui.small("Сетка появляется при масштабе от 600%");
        ui.add_space(10.0);
        ui.separator();
        ui.strong(if self.edit_target() == EditTarget::Mask {
            "Маска"
        } else {
            "Активный слой"
        });
        if self.edit_target() == EditTarget::Raster {
            ui.label(self.document.layers()[self.document.active()].name());
        }
        let locked = self.edit_target() == EditTarget::Raster
            && self.document.layers()[self.document.active()].locked();
        ui.add_enabled_ui(!locked, |ui| {
            ui.add(egui::Slider::new(&mut self.blur, 0.1..=20.0).text("Размытие"));
            if ui.button("Применить размытие").clicked() {
                let v = self.blur;
                self.edit_manual(|d| d.adjust(Adjustment::Blur(v)));
            }
            ui.horizontal(|ui| {
                if ui.button("Ч/Б").clicked() {
                    self.edit_manual(|d| d.adjust(Adjustment::Grayscale));
                }
                if ui.button("Убрать шум").clicked() {
                    self.edit_manual(|d| d.adjust(Adjustment::Median));
                }
            });
        });
        if locked {
            ui.weak("Пиксели слоя заблокированы");
        }
        ui.add_space(12.0);
        ui.separator();
        ui.strong("Клавиатура");
        ui.small("B — кисть · E — ластик · [ / ] — радиус\nX — поменять цвета · D — чёрный / белый\nQ — Растр / Маска\nCtrl+D — снять маску\nCtrl+J — копия слоя · Ctrl+E — объединить\nCtrl+Z / Ctrl+Shift+Z — отмена / повтор");
    }
    fn apply_manual_crop(&mut self) {
        let [x, y, w, h] = self.crop;
        self.edit(|d| d.crop(x, y, w, h));
        self.manual_ui.crop_pending = false;
        self.manual_ui.fit_once = true;
    }
    #[allow(clippy::too_many_lines)] // Gesture begin/update/end and cancellation stay in one dispatcher.
    pub(super) fn manual_canvas(&mut self, ui: &Ui, response: &Response) {
        if self.navigation.interacting || self.segmentation_point_mode() {
            self.pause_pasted_drag();
            self.finish_manual_stroke();
            self.manual_ui.start = None;
            self.manual_ui.lasso.clear();
            return;
        }
        let tool = self.manual_ui.tool;
        let position = response
            .interact_pointer_pos()
            .or_else(|| ui.input(|i| i.pointer.hover_pos()));
        let point = position.map(|p| {
            [
                (p.x - response.rect.left()) / self.zoom,
                (p.y - response.rect.top()) / self.zoom,
            ]
        });
        let down = ui.input(|i| i.pointer.primary_down());
        if self.manual_ui.ignore_until_release {
            if !down {
                self.manual_ui.ignore_until_release = false;
            }
            return;
        }
        if tool == ManualTool::Zoom {
            if response.clicked() {
                self.zoom = (self.zoom * 1.25).min(8.0);
            }
            if response.secondary_clicked() {
                self.zoom = (self.zoom / 1.25).max(0.02);
            }
            return;
        }
        if tool == ManualTool::Hand {
            self.pasted_canvas(ui, response, point);
            return;
        }
        if tool == ManualTool::Eyedropper {
            if response.clicked() {
                if let Some(p) = point {
                    let raster = if self.edit_target() == EditTarget::Mask {
                        self.document.mask_raster()
                    } else if self.manual_ui.merged_sample {
                        self.document.flatten()
                    } else {
                        self.document.layers()[self.document.active()]
                            .raster()
                            .clone()
                    };
                    if p[0] >= 0.0 && p[1] >= 0.0 {
                        if let Some(c) = raster.pixel(p[0] as u32, p[1] as u32) {
                            self.color = Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
                        }
                    }
                }
            }
            return;
        }
        if matches!(tool, ManualTool::Wand | ManualTool::ColorSelect) {
            if response.clicked() {
                if let Some(p) = point.filter(|p| p[0] >= 0.0 && p[1] >= 0.0) {
                    let sample = if self.edit_target() == EditTarget::Mask {
                        self.document.mask_raster()
                    } else if self.manual_ui.merged_sample {
                        self.document.flatten()
                    } else {
                        self.document.layers()[self.document.active()]
                            .raster()
                            .clone()
                    };
                    let tolerance = self.manual_ui.tolerance;
                    let op = self.selection_operation(ui);
                    self.edit(|d| {
                        d.select_by_color(
                            &sample,
                            [p[0] as u32, p[1] as u32],
                            tolerance,
                            tool == ManualTool::Wand,
                            op,
                        )
                    });
                }
            }
            return;
        }
        if matches!(tool, ManualTool::Clone | ManualTool::Heal) {
            self.retouch_canvas(ui, response, point, position, down);
            return;
        }
        if tool == ManualTool::Fill {
            if response.clicked() {
                if let Some(p) = point {
                    if p[0] >= 0.0 && p[1] >= 0.0 {
                        let color = self.color.to_srgba_unmultiplied();
                        let tolerance = self.manual_ui.tolerance;
                        let style = self.brush_style();
                        self.edit_manual(|d| {
                            d.bucket_fill(p[0] as u32, p[1] as u32, color, tolerance, style)
                        });
                    }
                }
            }
            return;
        }
        let engaged = (response.is_pointer_button_down_on() && down)
            || response.dragged_by(egui::PointerButton::Primary);
        if let Some(brush) = tool.brush() {
            if engaged || (!down && self.manual_ui.stroke_before.is_some()) {
                if let Some(p) = point {
                    if self.edit_target() == EditTarget::Raster
                        && self.document.layers()[self.document.active()].locked()
                        && matches!(brush, Tool::Brush | Tool::Eraser)
                    {
                        self.status = "Слой заблокирован".into();
                        return;
                    }
                    if self.manual_ui.stroke_before.is_none() {
                        self.manual_ui.stroke_before = Some((self.document.clone(), self.unsaved));
                    }
                    let style = self.brush_style();
                    let from = self.manual_ui.last.unwrap_or(p);
                    let radius = self.radius;
                    let color = self.color.to_srgba_unmultiplied();
                    let stroke = |d: &mut Document| {
                        d.stroke_with_style(from, p, radius, color, brush, style)
                    };
                    let result = if self.edit_target() == EditTarget::Mask {
                        self.document.edit_mask(stroke)
                    } else {
                        stroke(&mut self.document)
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
                }
            } else if !down {
                self.finish_manual_stroke();
            }
            if !down {
                self.finish_manual_stroke();
            }
            if response.hovered() {
                if let Some(pos) = position {
                    let painter = ui
                        .painter()
                        .with_clip_rect(response.rect.intersect(ui.clip_rect()));
                    painter.circle_stroke(
                        pos,
                        self.radius * self.zoom,
                        Stroke::new(2.0, Color32::BLACK),
                    );
                    painter.circle_stroke(
                        pos,
                        self.radius * self.zoom,
                        Stroke::new(1.0, Color32::WHITE),
                    );
                }
            }
            return;
        }
        if tool == ManualTool::Lasso {
            self.lasso_canvas(ui, response, point, engaged, down);
            return;
        }
        if engaged {
            if self.manual_ui.start.is_none() {
                self.manual_ui.selection_op = Some(self.selection_operation(ui));
            }
            if let Some(p) = point {
                self.manual_ui.start.get_or_insert(p);
                self.manual_ui.last = Some(p);
            }
        }
        if !down && self.manual_ui.start.is_some() {
            if let Some(point) = point {
                self.manual_ui.last = Some(point);
            }
        }
        if let (Some(start), Some(end)) = (self.manual_ui.start, self.manual_ui.last) {
            let p = |v: [f32; 2]| response.rect.min + Vec2::new(v[0], v[1]) * self.zoom;
            let painter = ui
                .painter()
                .with_clip_rect(response.rect.intersect(ui.clip_rect()));
            let rect = Rect::from_two_pos(p(start), p(end));
            let stroke = Stroke::new(1.5, Color32::WHITE);
            if matches!(
                tool,
                ManualTool::Line | ManualTool::Gradient | ManualTool::Move
            ) {
                painter.line_segment([p(start), p(end)], stroke);
            } else if tool == ManualTool::Ellipse {
                painter.add(egui::Shape::ellipse_stroke(
                    rect.center(),
                    rect.size() / 2.0,
                    stroke,
                ));
            } else {
                painter.rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Inside);
            }
            if !down {
                self.manual_ui.start = None;
                self.manual_ui.last = None;
                let color = self.color.to_srgba_unmultiplied();
                let background = self.manual_ui.background.to_srgba_unmultiplied();
                let width = self.manual_ui.width;
                let filled = self.manual_ui.filled;
                let mut style = self.brush_style();
                style.hardness = 1.0;
                match tool {
                    ManualTool::Move => {
                        let dx = (end[0] - start[0]).round() as i32;
                        let dy = (end[1] - start[1]).round() as i32;
                        self.edit_manual(|d| d.translate_layer(dx, dy));
                    }
                    ManualTool::Select => {
                        let op = self
                            .manual_ui
                            .selection_op
                            .take()
                            .unwrap_or_else(|| self.selection_operation(ui));
                        self.edit(|d| d.select_area_rectangle(start, end, op));
                    }
                    ManualTool::Gradient => {
                        self.edit_manual(|d| d.gradient(start, end, color, background, style));
                    }
                    ManualTool::Line | ManualTool::Rectangle | ManualTool::Ellipse => {
                        let shape = match tool {
                            ManualTool::Line => Shape::Line,
                            ManualTool::Rectangle => Shape::Rectangle,
                            _ => Shape::Ellipse,
                        };
                        self.edit_manual(|d| {
                            d.draw_shape(start, end, shape, width, filled, color, style)
                        });
                    }
                    ManualTool::Crop => {
                        let x = start[0]
                            .min(end[0])
                            .floor()
                            .clamp(0.0, self.document.width() as f32)
                            as u32;
                        let y = start[1]
                            .min(end[1])
                            .floor()
                            .clamp(0.0, self.document.height() as f32)
                            as u32;
                        let right = start[0]
                            .max(end[0])
                            .ceil()
                            .clamp(0.0, self.document.width() as f32)
                            as u32;
                        let bottom = start[1]
                            .max(end[1])
                            .ceil()
                            .clamp(0.0, self.document.height() as f32)
                            as u32;
                        self.crop = [x, y, right - x, bottom - y];
                        self.manual_ui.crop_pending = right > x && bottom > y;
                    }
                    _ => {}
                }
            }
        }
        if tool == ManualTool::Crop && self.manual_ui.crop_pending {
            let [x, y, w, h] = self.crop;
            let rect = Rect::from_min_size(
                response.rect.min + Vec2::new(x as f32, y as f32) * self.zoom,
                Vec2::new(w as f32, h as f32) * self.zoom,
            );
            ui.painter()
                .with_clip_rect(response.rect.intersect(ui.clip_rect()))
                .rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(2.0, ACCENT),
                    egui::StrokeKind::Inside,
                );
        }
    }
}

fn action_icon(p: &egui::Painter, r: Rect, kind: usize, color: Color32) {
    let s = Stroke::new(1.5, color);
    let a = |x: f32, y: f32| r.min + Vec2::new(x * r.width(), y * r.height());
    match kind {
        0 => {
            p.line_segment([a(0.0, 0.5), a(1.0, 0.5)], s);
            p.line_segment([a(0.5, 0.0), a(0.5, 1.0)], s);
        }
        1 => {
            for offset in [0.0, 0.28] {
                p.rect_stroke(
                    Rect::from_min_max(a(offset, offset), a(offset + 0.72, offset + 0.72)),
                    0.0,
                    s,
                    egui::StrokeKind::Inside,
                );
            }
        }
        2..=4 => {
            let (top, bottom) = if kind == 3 { (1.0, 0.0) } else { (0.0, 0.8) };
            p.line_segment([a(0.5, top), a(0.5, bottom)], s);
            let wing_y = if kind == 3 { 0.35 } else { 0.45 };
            p.line_segment([a(0.15, wing_y), a(0.5, bottom)], s);
            p.line_segment([a(0.85, wing_y), a(0.5, bottom)], s);
            if kind == 2 {
                p.line_segment([a(0.0, 1.0), a(1.0, 1.0)], s);
            }
        }
        5 => {
            p.rect_stroke(
                Rect::from_min_max(a(0.2, 0.2), a(0.8, 1.0)),
                1.0,
                s,
                egui::StrokeKind::Inside,
            );
            p.line_segment([a(0.05, 0.2), a(0.95, 0.2)], s);
            p.line_segment([a(0.35, 0.0), a(0.65, 0.0)], s);
        }
        _ => {
            p.line_segment([a(0.0, 0.25), a(1.0, 0.25)], s);
            p.line_segment([a(0.7, 0.0), a(1.0, 0.25)], s);
            p.line_segment([a(0.0, 0.75), a(1.0, 0.75)], s);
            p.line_segment([a(0.0, 0.75), a(0.3, 1.0)], s);
        }
    }
}

fn checker(painter: &egui::Painter, rect: Rect, cell: f32) {
    painter.rect_filled(rect, 2.0, Color32::from_gray(72));
    let p = painter.with_clip_rect(rect);
    for y in 0..(rect.height() / cell).ceil() as u32 {
        for x in 0..(rect.width() / cell).ceil() as u32 {
            if (x + y) % 2 == 0 {
                p.rect_filled(
                    Rect::from_min_size(
                        rect.min + Vec2::new(x as f32, y as f32) * cell,
                        Vec2::splat(cell),
                    ),
                    0.0,
                    Color32::from_gray(95),
                );
            }
        }
    }
}
fn eye_icon(p: &egui::Painter, r: Rect, visible: bool) {
    let s = Stroke::new(
        1.4,
        if visible {
            Color32::from_gray(216)
        } else {
            Color32::from_gray(105)
        },
    );
    p.add(egui::Shape::closed_line(
        vec![
            egui::pos2(r.left(), r.center().y),
            r.center_top(),
            egui::pos2(r.right(), r.center().y),
            r.center_bottom(),
        ],
        s,
    ));
    p.circle_stroke(r.center(), 2.0, s);
    if !visible {
        p.line_segment([r.left_bottom(), r.right_top()], s);
    }
}
fn lock_icon(p: &egui::Painter, r: Rect, locked: bool) {
    let s = Stroke::new(
        1.4,
        if locked {
            Color32::from_gray(235)
        } else {
            Color32::from_gray(104)
        },
    );
    p.rect_stroke(
        Rect::from_min_max(egui::pos2(r.left(), r.center().y), r.right_bottom()),
        1.0,
        s,
        egui::StrokeKind::Inside,
    );
    let top = Rect::from_min_max(
        r.left_top() + Vec2::new(if locked { 3.0 } else { 6.0 }, 0.0),
        egui::pos2(r.right() - 2.0, r.center().y + 2.0),
    );
    p.rect_stroke(top, 4.0, s, egui::StrokeKind::Inside);
}
#[allow(clippy::too_many_lines)] // Tool glyphs share the same local coordinate projection.
fn icon(p: &egui::Painter, r: Rect, tool: ManualTool, color: Color32) {
    let s = Stroke::new(1.6, color);
    let a = |x: f32, y: f32| r.min + Vec2::new(x * r.width(), y * r.height());
    match tool {
        ManualTool::Move => {
            p.line_segment([a(0.1, 0.5), a(0.9, 0.5)], s);
            p.line_segment([a(0.5, 0.1), a(0.5, 0.9)], s);
            for v in [
                vec![a(0.3, 0.3), a(0.5, 0.1), a(0.7, 0.3)],
                vec![a(0.3, 0.7), a(0.5, 0.9), a(0.7, 0.7)],
                vec![a(0.3, 0.3), a(0.1, 0.5), a(0.3, 0.7)],
                vec![a(0.7, 0.3), a(0.9, 0.5), a(0.7, 0.7)],
            ] {
                p.add(egui::Shape::line(v, s));
            }
        }
        ManualTool::Brush | ManualTool::Mask => {
            p.add(egui::Shape::closed_line(
                vec![a(0.2, 0.7), a(0.65, 0.05), a(0.9, 0.25), a(0.4, 0.85)],
                s,
            ));
            p.add(egui::Shape::line(
                vec![a(0.2, 0.7), a(0.05, 0.95), a(0.4, 0.85)],
                s,
            ));
            if tool == ManualTool::Mask {
                p.circle_stroke(a(0.8, 0.8), r.width() * 0.15, s);
            }
        }
        ManualTool::Eraser | ManualTool::Unmask => {
            p.add(egui::Shape::closed_line(
                vec![
                    a(0.05, 0.6),
                    a(0.55, 0.05),
                    a(0.95, 0.4),
                    a(0.45, 0.95),
                    a(0.3, 0.95),
                ],
                s,
            ));
            p.line_segment([a(0.3, 0.35), a(0.7, 0.7)], s);
            if tool == ManualTool::Unmask {
                p.line_segment([a(0.05, 0.1), a(0.95, 0.95)], s);
            }
        }
        ManualTool::Wand | ManualTool::ColorSelect => {
            p.line_segment([r.left_bottom(), r.right_top()], s);
            p.circle_stroke(r.right_top(), 3.0, s);
            if tool == ManualTool::ColorSelect {
                p.circle_filled(r.left_top(), 2.5, color);
            }
        }
        ManualTool::Clone | ManualTool::Heal => {
            p.circle_stroke(r.center_top() + Vec2::new(0.0, 4.0), 3.0, s);
            p.rect_stroke(
                Rect::from_min_max(r.left_bottom() - Vec2::new(0.0, 5.0), r.right_bottom()),
                1.0,
                s,
                egui::StrokeKind::Inside,
            );
            if tool == ManualTool::Heal {
                p.line_segment(
                    [
                        r.center() - Vec2::new(3.0, 0.0),
                        r.center() + Vec2::new(3.0, 0.0),
                    ],
                    s,
                );
            }
        }
        ManualTool::Lasso => {
            p.add(egui::Shape::closed_line(
                vec![
                    a(0.15, 0.1),
                    a(0.85, 0.15),
                    a(0.95, 0.6),
                    a(0.6, 0.9),
                    a(0.1, 0.65),
                ],
                s,
            ));
            p.line_segment([a(0.15, 0.65), a(0.0, 1.0)], s);
        }
        ManualTool::Select => {
            for (a, b) in [
                (a(0.05, 0.05), a(0.4, 0.05)),
                (a(0.6, 0.05), a(0.95, 0.05)),
                (a(0.05, 0.95), a(0.4, 0.95)),
                (a(0.6, 0.95), a(0.95, 0.95)),
                (a(0.05, 0.05), a(0.05, 0.4)),
                (a(0.05, 0.6), a(0.05, 0.95)),
                (a(0.95, 0.05), a(0.95, 0.4)),
                (a(0.95, 0.6), a(0.95, 0.95)),
            ] {
                p.line_segment([a, b], s);
            }
        }
        ManualTool::Rectangle => {
            p.rect_stroke(r.shrink(1.0), 1.0, s, egui::StrokeKind::Inside);
        }
        ManualTool::Ellipse => {
            p.add(egui::Shape::ellipse_stroke(r.center(), r.size() * 0.45, s));
        }
        ManualTool::Line => {
            p.line_segment([a(0.1, 0.9), a(0.9, 0.1)], s);
            p.circle_filled(a(0.1, 0.9), 2.0, color);
            p.circle_filled(a(0.9, 0.1), 2.0, color);
        }
        ManualTool::Gradient => {
            p.rect_stroke(r, 1.0, s, egui::StrokeKind::Inside);
            for i in 0..5 {
                p.rect_filled(
                    Rect::from_min_max(a(i as f32 / 5.0, 0.0), a((i + 1) as f32 / 5.0, 1.0)),
                    0.0,
                    color.gamma_multiply((i + 1) as f32 / 5.0),
                );
            }
        }
        ManualTool::Fill => {
            p.add(egui::Shape::closed_line(
                vec![a(0.05, 0.5), a(0.45, 0.05), a(0.85, 0.45), a(0.4, 0.9)],
                s,
            ));
            p.line_segment([a(0.25, 0.15), a(0.65, 0.55)], s);
            p.circle_filled(a(0.9, 0.85), 2.5, color);
        }
        ManualTool::Eyedropper => {
            p.add(egui::Shape::closed_line(
                vec![a(0.1, 0.85), a(0.7, 0.15), a(0.85, 0.3), a(0.25, 0.95)],
                s,
            ));
            p.line_segment([a(0.5, 0.1), a(0.95, 0.5)], s);
        }
        ManualTool::Crop => {
            p.add(egui::Shape::line(
                vec![a(0.25, 0.0), a(0.25, 0.75), a(1.0, 0.75)],
                s,
            ));
            p.add(egui::Shape::line(
                vec![a(0.0, 0.25), a(0.75, 0.25), a(0.75, 1.0)],
                s,
            ));
        }
        ManualTool::Hand => {
            p.add(egui::Shape::closed_line(
                vec![
                    a(0.2, 0.6),
                    a(0.1, 0.35),
                    a(0.25, 0.3),
                    a(0.35, 0.5),
                    a(0.35, 0.05),
                    a(0.48, 0.05),
                    a(0.5, 0.4),
                    a(0.6, 0.15),
                    a(0.72, 0.2),
                    a(0.7, 0.5),
                    a(0.9, 0.35),
                    a(0.95, 0.5),
                    a(0.8, 0.9),
                    a(0.4, 0.95),
                ],
                s,
            ));
        }
        ManualTool::Zoom => {
            p.circle_stroke(a(0.4, 0.4), r.width() * 0.33, s);
            p.line_segment([a(0.65, 0.65), a(0.95, 0.95)], s);
            p.line_segment([a(0.2, 0.4), a(0.6, 0.4)], s);
            p.line_segment([a(0.4, 0.2), a(0.4, 0.6)], s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ctx: &egui::Context, editor: &mut ImageEditor, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(200.0))),
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                editor.manual_shortcuts(ui);
                let (_, response) =
                    ui.allocate_exact_size(Vec2::splat(64.0), egui::Sense::click_and_drag());
                editor.manual_canvas(ui, &response);
            });
        });
    }
    fn pointer(x: f32, y: f32, pressed: bool) -> Vec<egui::Event> {
        let pos = egui::pos2(x, y);
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }
    #[test]
    fn drag_shapes_and_brushes_make_one_undo_including_release_endpoint() {
        for tool in [ManualTool::Rectangle, ManualTool::Brush] {
            let ctx = egui::Context::default();
            let mut e = ImageEditor::new(Document::new(64, 64).unwrap());
            e.zoom = 1.0;
            e.select_manual_tool(tool);
            e.manual_ui.filled = true;
            e.color = Color32::RED;
            e.radius = 1.0;
            let original = e.document.clone();
            frame(&ctx, &mut e, vec![]);
            frame(&ctx, &mut e, pointer(16.0, 16.0, true));
            frame(
                &ctx,
                &mut e,
                vec![egui::Event::PointerMoved(egui::pos2(30.0, 30.0))],
            );
            frame(&ctx, &mut e, pointer(50.0, 50.0, false));
            assert_ne!(e.document, original);
            assert!(
                e.document.flatten().pixel(40, 40).unwrap()[3] > 0,
                "release endpoint"
            );
            let changed = e.document.clone();
            e.restore_history(false);
            assert_eq!(e.document, original);
            assert!(
                !e.history.undo(&mut e.document),
                "one gesture, one checkpoint"
            );
            e.restore_history(true);
            assert_eq!(e.document, changed);
        }
    }
    #[test]
    fn lasso_pointer_gesture_selects_once_and_escape_cancels_without_changes() {
        let ctx = egui::Context::default();
        let mut editor = ImageEditor::new(Document::new(32, 32).unwrap());
        editor.select_manual_tool(ManualTool::Lasso);
        frame(&ctx, &mut editor, vec![]);
        frame(&ctx, &mut editor, pointer(12.0, 12.0, true));
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerMoved(egui::pos2(30.0, 12.0))],
        );
        frame(&ctx, &mut editor, pointer(12.0, 30.0, false));
        assert!(
            editor
                .document
                .selection()
                .iter()
                .filter(|v| **v > 0)
                .count()
                > 50
        );
        editor.restore_history(false);
        assert!(editor.document.selection().iter().all(|v| *v == 0));
        let before = editor.document.clone();
        frame(&ctx, &mut editor, pointer(12.0, 12.0, true));
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::Key {
                key: Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        frame(&ctx, &mut editor, pointer(30.0, 30.0, false));
        assert_eq!(editor.document, before);
    }
    #[test]
    fn escape_cancels_stroke_until_mouse_release() {
        let ctx = egui::Context::default();
        let mut e = ImageEditor::new(Document::new(64, 64).unwrap());
        e.zoom = 1.0;
        let original = e.document.clone();
        frame(&ctx, &mut e, vec![]);
        frame(&ctx, &mut e, pointer(20.0, 20.0, true));
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
        frame(
            &ctx,
            &mut e,
            vec![egui::Event::PointerMoved(egui::pos2(40.0, 40.0))],
        );
        frame(&ctx, &mut e, pointer(45.0, 45.0, false));
        assert_eq!(e.document, original);
        assert!(!e.has_unsaved_changes());
        assert!(!e.history.undo(&mut e.document));
    }
    #[test]
    fn layer_commands_are_reversible_and_failed_delete_keeps_selection() {
        let mut e = ImageEditor::new(Document::new(8, 8).unwrap());
        e.layer_action(LayerAction::New);
        e.layer_action(LayerAction::Rename(1, "Ink".into()));
        e.layer_action(LayerAction::Lock(1));
        e.layer_action(LayerAction::Select(0));
        let before = e.document.clone();
        e.layer_action(LayerAction::Remove(1));
        assert_eq!(e.document, before);
        e.layer_action(LayerAction::Duplicate(1));
        assert!(!e.document.layers()[2].locked());
        e.restore_history(false);
        assert_eq!(e.document, before);
    }
    #[test]
    fn selection_crop_move_gradient_and_sampling_use_canvas_coordinates() {
        for tool in [
            ManualTool::Select,
            ManualTool::Crop,
            ManualTool::Move,
            ManualTool::Gradient,
            ManualTool::Line,
            ManualTool::Ellipse,
            ManualTool::Fill,
            ManualTool::Eyedropper,
            ManualTool::Mask,
            ManualTool::Unmask,
        ] {
            let ctx = egui::Context::default();
            let mut doc =
                Document::from_raster(Raster::solid(64, 64, [120, 60, 30, 255]).unwrap()).unwrap();
            doc.select_rectangle([0.0; 2], [64.0; 2]).unwrap();
            let mut e = ImageEditor::new(doc);
            e.zoom = 1.0;
            e.color = Color32::BLUE;
            e.select_manual_tool(tool);
            let original = e.document.clone();
            frame(&ctx, &mut e, vec![]);
            frame(&ctx, &mut e, pointer(16.0, 16.0, true));
            if !matches!(tool, ManualTool::Fill | ManualTool::Eyedropper) {
                frame(
                    &ctx,
                    &mut e,
                    vec![egui::Event::PointerMoved(egui::pos2(35.0, 35.0))],
                );
            }
            let end = if matches!(tool, ManualTool::Fill | ManualTool::Eyedropper) {
                16.0
            } else {
                48.0
            };
            frame(&ctx, &mut e, pointer(end, end, false));
            if tool == ManualTool::Crop {
                assert!(e.manual_ui.crop_pending);
                assert_eq!(e.document, original);
                e.apply_manual_crop();
                assert_eq!(e.document.width(), 32);
            } else if tool == ManualTool::Eyedropper {
                assert_eq!(e.color, Color32::from_rgb(120, 60, 30));
                assert_eq!(e.document, original);
                continue;
            } else if tool == ManualTool::Mask {
                // Painting white over an already white mask is a no-op.
                assert_eq!(e.document, original);
                continue;
            }
            assert_ne!(e.document, original, "{tool:?}");
            e.restore_history(false);
            assert_eq!(e.document, original, "{tool:?}");
        }
    }
    #[test]
    fn clipboard_controls_keep_layer_actions_visible_in_a_small_dock() {
        let ctx = egui::Context::default();
        super::super::apply_studio_style(&ctx);
        let mut editor = ImageEditor::new(Document::new(8, 8).unwrap());
        editor.copy_pixels(false);
        for _ in 0..2 {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(320.0, 580.0))),
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default()
                        .show(ui, |ui| editor.manual_dock(ui, &mut vec![]));
                },
            );
        }
        let row = ctx
            .read_response(egui::Id::new(("layer-row", 0_usize)))
            .expect("visible layer row");
        assert!(
            row.rect.bottom() < 500.0,
            "layer action bar must fit below: {:?}",
            row.rect
        );
    }
    fn dock_frame(ctx: &egui::Context, editor: &mut ImageEditor, events: Vec<egui::Event>) {
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(340.0, 700.0))),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| editor.layers_dock(ui));
            },
        );
    }
    #[test]
    fn layer_row_eye_lock_and_drag_are_operable_with_pointer_events() {
        let ctx = egui::Context::default();
        let mut e = ImageEditor::new(Document::new(8, 8).unwrap());
        e.layer_action(LayerAction::New);
        e.layer_action(LayerAction::Rename(1, "Ink".into()));
        dock_frame(&ctx, &mut e, vec![]);
        let top = ctx
            .read_response(egui::Id::new(("layer-row", 1_usize)))
            .unwrap()
            .rect;
        let eye = top.min + egui::vec2(17.0, 29.0);
        dock_frame(&ctx, &mut e, pointer(eye.x, eye.y, true));
        dock_frame(&ctx, &mut e, pointer(eye.x, eye.y, false));
        assert!(!e.document.layers()[1].visible(), "eye button");
        let lock = egui::pos2(top.right() - 17.0, top.top() + 29.0);
        dock_frame(&ctx, &mut e, pointer(lock.x, lock.y, true));
        dock_frame(&ctx, &mut e, pointer(lock.x, lock.y, false));
        assert!(e.document.layers()[1].locked(), "lock button");
        let bottom = ctx
            .read_response(egui::Id::new(("layer-row", 0_usize)))
            .unwrap()
            .rect;
        let source = top.center();
        let target = egui::pos2(bottom.center().x, bottom.bottom() - 4.0);
        dock_frame(&ctx, &mut e, pointer(source.x, source.y, true));
        dock_frame(&ctx, &mut e, vec![egui::Event::PointerMoved(target)]);
        dock_frame(&ctx, &mut e, vec![]);
        dock_frame(&ctx, &mut e, pointer(target.x, target.y, false));
        assert_eq!(
            e.document.layers()[0].name(),
            "Ink",
            "drag below bottom row"
        );
        e.restore_history(false);
        assert_eq!(e.document.layers()[1].name(), "Ink");
    }
    #[test]
    fn same_tools_edit_the_selected_target_with_one_undo_and_mask_ignores_layer_lock() {
        for target in [EditTarget::Raster, EditTarget::Mask] {
            for tool in [
                ManualTool::Brush,
                ManualTool::Eraser,
                ManualTool::Fill,
                ManualTool::Gradient,
                ManualTool::Line,
                ManualTool::Rectangle,
                ManualTool::Ellipse,
                ManualTool::Move,
            ] {
                let ctx = egui::Context::default();
                let mut doc =
                    Document::from_raster(Raster::solid(64, 64, [80, 100, 120, 255]).unwrap())
                        .unwrap();
                doc.select_rectangle([0.0, 0.0], [64.0, 64.0]).unwrap();
                if target == EditTarget::Mask {
                    doc.set_layer_locked(true);
                }
                let mut e = ImageEditor::new(doc);
                e.zoom = 1.0;
                e.set_edit_target(target);
                e.select_manual_tool(tool);
                e.color = Color32::from_gray(100);
                e.manual_ui.filled = true;
                let before = e.document.clone();
                frame(&ctx, &mut e, vec![]);
                frame(&ctx, &mut e, pointer(16.0, 16.0, true));
                if tool != ManualTool::Fill {
                    frame(
                        &ctx,
                        &mut e,
                        vec![egui::Event::PointerMoved(egui::pos2(35.0, 35.0))],
                    );
                }
                let end = if tool == ManualTool::Fill { 16.0 } else { 48.0 };
                frame(&ctx, &mut e, pointer(end, end, false));
                assert_ne!(e.document, before, "{target:?} {tool:?}");
                if target == EditTarget::Mask {
                    assert_eq!(e.document.layers(), before.layers(), "{tool:?}");
                } else {
                    assert_eq!(e.document.mask(), before.mask(), "{tool:?}");
                }
                e.restore_history(false);
                assert_eq!(e.document, before, "{target:?} {tool:?}");
            }
        }
    }
    #[test]
    fn target_switch_preserves_tool_colors_document_and_picks_mask_coverage() {
        let ctx = egui::Context::default();
        let mut e = ImageEditor::new(
            Document::from_raster(Raster::solid(64, 64, [80, 100, 120, 255]).unwrap()).unwrap(),
        );
        e.document.set_mask(vec![77; 4096]).unwrap();
        e.color = Color32::RED;
        e.manual_ui.background = Color32::BLUE;
        let before = e.document.clone();
        e.select_manual_tool(ManualTool::Eyedropper);
        e.set_edit_target(EditTarget::Mask);
        assert_eq!(e.manual_ui.tool, ManualTool::Eyedropper);
        assert_eq!(e.color, Color32::WHITE);
        assert_eq!(e.manual_ui.background, Color32::BLACK);
        frame(&ctx, &mut e, vec![]);
        frame(&ctx, &mut e, pointer(16.0, 16.0, true));
        frame(&ctx, &mut e, pointer(16.0, 16.0, false));
        assert_eq!(e.color, Color32::from_gray(77));
        assert_eq!(e.document, before);
        e.set_edit_target(EditTarget::Raster);
        assert_eq!(e.color, Color32::RED);
        assert_eq!(e.manual_ui.background, Color32::BLUE);
        e.set_edit_target(EditTarget::Mask);
        assert_eq!(e.color, Color32::from_gray(77));
        assert_eq!(e.document, before);
        assert!(!e.history.undo(&mut e.document));
    }
    #[test]
    fn selection_mode_is_captured_at_press_and_does_not_replace_ai_mask() {
        let ctx = egui::Context::default();
        let mut e = ImageEditor::new(Document::new(64, 64).unwrap());
        e.document.set_mask(vec![77; 4096]).unwrap();
        e.document
            .select_area_rectangle([0.0, 0.0], [4.0, 4.0], crate::SelectionOp::Replace)
            .unwrap();
        e.select_manual_tool(ManualTool::Select);
        e.zoom = 1.0;
        e.ribbon_ui.selection_op = crate::SelectionOp::Add;
        frame(&ctx, &mut e, vec![]);
        frame(&ctx, &mut e, pointer(20.0, 20.0, true));
        e.ribbon_ui.selection_op = crate::SelectionOp::Subtract;
        frame(
            &ctx,
            &mut e,
            vec![egui::Event::PointerMoved(egui::pos2(36.0, 36.0))],
        );
        frame(&ctx, &mut e, pointer(36.0, 36.0, false));
        assert_eq!(e.document.selection()[0], 255);
        assert_eq!(e.document.selection()[20 * 64 + 20], 255);
        assert_eq!(e.document.mask(), &[77; 4096]);
    }
}
