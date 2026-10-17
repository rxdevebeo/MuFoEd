//! The egui widget: a ribbon of structures over the formula, in the same
//! office colours the host already uses.

use egui::{
    Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Id, Key, Margin, Modifiers, Popup,
    PopupCloseBehavior, RichText, ScrollArea, Sense, Stroke, TextEdit, Vec2,
};

use crate::edit::{self, insert_str};
use crate::icons::{self, Accent, Bracket, Icon};
use crate::layout::{self, Look};
use crate::mathml::{self, MathmlError};
use crate::model::{Caret, Formula};
use crate::text::Text;

/// How the formula is drawn. The italic family is the one the host installed;
/// `MuFoEd` registers Arimo italic under the name `italic`.
#[derive(Clone, Debug)]
pub struct Style {
    /// Family for single-letter identifiers.
    pub italic_family: FontFamily,
    /// Em size of the base line, in points.
    pub base_size: f32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            italic_family: FontFamily::Proportional,
            base_size: 28.0,
        }
    }
}

/// A visual editor of one Presentation `MathML` formula.
///
/// Show it from any egui `Ui`. [`Self::mathml`] is what the host stores;
/// `MuFoEd` can turn that string into a document formula with the library's
/// `MathML` reader, without this crate knowing about OOXML.
#[derive(Clone, Debug)]
pub struct MathEditor {
    formula: Formula,
    caret: Caret,
    /// Where a shift-selection starts.
    anchor: Caret,
    selection: Option<edit::Selection>,
    undo: Vec<(Formula, Caret)>,
    redo: Vec<(Formula, Caret)>,
    source: String,
    source_edited: bool,
    parse_error: Option<String>,
    show_source: bool,
    style: Style,
    text: Text,
    focus: Option<Id>,
    want_focus: bool,
}

impl Default for MathEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl MathEditor {
    /// An empty formula, with the caret in it.
    #[must_use]
    pub fn new() -> Self {
        Self::from_formula(Formula::default())
    }

    /// The formula `source` describes.
    ///
    /// # Errors
    ///
    /// [`MathmlError`] when `source` is not a `MathML` `math` element this
    /// editor can read.
    pub fn from_mathml(source: &str) -> Result<Self, MathmlError> {
        Ok(Self::from_formula(mathml::parse(source)?))
    }

    fn from_formula(formula: Formula) -> Self {
        let source = mathml::write(&formula);
        Self {
            formula,
            caret: Caret::default(),
            anchor: Caret::default(),
            selection: None,
            undo: Vec::new(),
            redo: Vec::new(),
            source,
            source_edited: false,
            parse_error: None,
            show_source: false,
            style: Style::default(),
            text: Text::english(),
            focus: None,
            want_focus: true,
        }
    }

    /// Presentation `MathML` for the formula as it stands.
    #[must_use]
    pub fn mathml(&self) -> String {
        mathml::write(&self.formula)
    }

    /// Replace the formula.
    ///
    /// # Errors
    ///
    /// [`MathmlError`] when `source` cannot be read. The current formula
    /// stays as it was.
    pub fn set_mathml(&mut self, source: &str) -> Result<(), MathmlError> {
        self.formula = mathml::parse(source)?;
        self.caret = Caret::default();
        self.anchor = Caret::default();
        self.selection = None;
        self.source = mathml::write(&self.formula);
        self.source_edited = false;
        self.parse_error = None;
        Ok(())
    }

    /// The font and size. `MuFoEd` should set [`Style::italic_family`] to the
    /// italic face it installed for the interface.
    pub fn style_mut(&mut self) -> &mut Style {
        &mut self.style
    }

    /// Tooltips, the source button, the window title and the heading.
    ///
    /// The built-in text is English. [`Self::set_language`] switches to the
    /// built-in Russian catalog; [`Self::text_mut`] replaces any single string
    /// with the host's own translation.
    #[must_use]
    pub fn text(&self) -> &Text {
        &self.text
    }

    /// The strings [`Self::text`] returns, for a host that translates them.
    pub fn text_mut(&mut self) -> &mut Text {
        &mut self.text
    }

    /// Replace every interface string.
    pub fn set_text(&mut self, text: Text) {
        self.text = text;
    }

    /// Use the built-in catalog for `code` (`ru` or anything else).
    pub fn set_language(&mut self, code: &str) {
        self.text = Text::for_code(code);
    }

    /// Show the `MathML` source under the formula, with a button that applies it.
    pub fn set_show_source(&mut self, show: bool) {
        self.show_source = show;
    }

    /// Draw the toolbar and the formula. `true` when the formula changed.
    pub fn show(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        if let Some(action) = Self::toolbar(ui, &self.text) {
            changed = true;
            match action {
                Action::Undo => self.undo(),
                Action::Redo => self.redo(),
                other => {
                    self.snapshot();
                    self.delete_selection();
                    self.apply(other);
                }
            }
            if let Some(id) = self.focus {
                ui.memory_mut(|memory| memory.request_focus(id));
            }
        }
        if self.canvas(ui) {
            changed = true;
        }
        if changed {
            self.source_edited = false;
        }
        if !self.source_edited {
            self.source = mathml::write(&self.formula);
        }
        if self.show_source && self.source_panel(ui) {
            changed = true;
        }
        changed
    }

    fn snapshot(&mut self) {
        self.undo.push((self.formula.clone(), self.caret.clone()));
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn undo(&mut self) {
        if let Some(state) = self.undo.pop() {
            self.redo.push((self.formula.clone(), self.caret.clone()));
            self.formula = state.0;
            self.caret = state.1.clone();
            self.anchor = state.1;
            self.selection = None;
        }
    }

    fn redo(&mut self) {
        if let Some(state) = self.redo.pop() {
            self.undo.push((self.formula.clone(), self.caret.clone()));
            self.formula = state.0;
            self.caret = state.1.clone();
            self.anchor = state.1;
            self.selection = None;
        }
    }

    fn delete_selection(&mut self) {
        let Some(selection) = self.selection.take() else {
            return;
        };
        edit::delete_selection(&mut self.formula, &selection, &mut self.caret);
        self.anchor.clone_from(&self.caret);
    }

    fn toolbar(ui: &mut egui::Ui, text: &Text) -> Option<Action> {
        let mut action = None;
        Frame::new()
            .fill(ui.visuals().panel_fill)
            .inner_margin(Margin::same(6))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().button_padding = Vec2::new(6.0, 3.0);
                    ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                    action = tool_row(ui, text);
                });
            });
        action
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::Undo | Action::Redo => {}
            Action::Fraction => edit::insert_fraction(&mut self.formula, &mut self.caret),
            Action::Sqrt => edit::insert_sqrt(&mut self.formula, &mut self.caret),
            Action::Root => edit::insert_root(&mut self.formula, &mut self.caret),
            Action::Sup => edit::insert_script(&mut self.formula, &mut self.caret, false, true),
            Action::Sub => edit::insert_script(&mut self.formula, &mut self.caret, true, false),
            Action::Fenced { open, close } => {
                edit::insert_fenced(&mut self.formula, &mut self.caret, open, close);
            }
            Action::Matrix {
                rows,
                columns,
                open,
                close,
            } => edit::insert_matrix(
                &mut self.formula,
                &mut self.caret,
                rows,
                columns,
                open,
                close,
            ),
            Action::Sum => edit::insert_limits(&mut self.formula, &mut self.caret, "∑", true, true),
            Action::Integral => {
                edit::insert_limits(&mut self.formula, &mut self.caret, "∫", true, true);
            }
            Action::Product => {
                edit::insert_limits(&mut self.formula, &mut self.caret, "∏", true, true);
            }
            Action::Over(symbol) => edit::insert_over(&mut self.formula, &mut self.caret, symbol),
            Action::Lim => {
                edit::insert_limits(&mut self.formula, &mut self.caret, "lim", true, false);
            }
            Action::Func(name) => edit::insert_function(&mut self.formula, &mut self.caret, name),
            Action::Symbol(symbol) => {
                edit::insert_symbol(&mut self.formula, &mut self.caret, &symbol);
            }
        }
        crate::model::clamp_caret(&self.formula, &mut self.caret);
    }

    fn canvas(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let height = if self.show_source {
            (ui.available_height() * 0.62).max(140.0)
        } else {
            ui.available_height().max(140.0)
        };
        Frame::new()
            .fill(paper(ui))
            .stroke(Stroke::new(
                1.0,
                ui.visuals().widgets.inactive.bg_stroke.color,
            ))
            .corner_radius(CornerRadius::same(4))
            .inner_margin(Margin::same(8))
            .show(ui, |ui| {
                ScrollArea::both()
                    .max_height(height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.paint_formula(ui) {
                            changed = true;
                        }
                    });
            });
        changed
    }

    fn paint_formula(&mut self, ui: &mut egui::Ui) -> bool {
        let focused = self
            .focus
            .is_some_and(|id| ui.memory(|memory| memory.has_focus(id)));
        let mut changed = false;
        if focused {
            changed = self.keys(ui);
        }
        let look = Look {
            italic: self.style.italic_family.clone(),
            size: self.style.base_size,
            ink: ui.visuals().text_color(),
            accent: ui.visuals().selection.bg_fill,
        };
        let caret_on = focused && ui.input(|input| input.time * 1.6).fract() < 0.55;
        let size = layout::measure(ui, &self.formula.root, &look);
        let pad = 18.0;
        let width = (size.x + pad * 2.0).max(ui.available_width());
        let content_h = size.y + pad * 2.0;
        let viewport = ui.available_height();
        let height = if viewport.is_finite() {
            content_h.max(viewport)
        } else {
            content_h.max(96.0)
        };
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click_and_drag());
        let mut hits = Vec::new();
        let origin = rect.min + egui::vec2(pad, (rect.height() - size.y).max(pad) * 0.5);
        layout::paint(
            ui,
            ui.painter(),
            origin,
            rect,
            &self.formula.root,
            &look,
            &self.caret,
            self.selection.as_ref(),
            &mut hits,
            caret_on,
        );
        self.focus = Some(response.id);
        if self.want_focus {
            response.request_focus();
            self.want_focus = false;
        }
        self.pointer(ui, &response, &hits);
        if focused {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(400));
        }
        response.on_hover_cursor(egui::CursorIcon::Text);
        changed
    }

    fn pointer(&mut self, ui: &egui::Ui, response: &egui::Response, hits: &[layout::Hit]) {
        let Some(pointer) = response.interact_pointer_pos() else {
            return;
        };
        let Some(caret) = layout::pick(hits, pointer) else {
            return;
        };
        let shift = ui.input(|input| input.modifiers.shift);
        if response.drag_started() {
            response.request_focus();
            if !shift {
                self.anchor.clone_from(&caret);
                self.selection = None;
            }
            self.caret = caret;
        } else if response.dragged() {
            self.selection = edit::selection_between(&self.anchor, &caret);
            self.caret = caret;
        } else if response.clicked() {
            response.request_focus();
            if shift {
                self.selection = edit::selection_between(&self.anchor, &caret);
            } else {
                self.anchor.clone_from(&caret);
                self.selection = None;
            }
            self.caret = caret;
        }
    }

    fn keys(&mut self, ui: &mut egui::Ui) -> bool {
        let command = Modifiers::COMMAND;
        if ui.input_mut(|input| input.consume_key(command, Key::Z)) {
            self.undo();
            return true;
        }
        if ui.input_mut(|input| input.consume_key(command | Modifiers::SHIFT, Key::Z))
            || ui.input_mut(|input| input.consume_key(command, Key::Y))
        {
            self.redo();
            return true;
        }
        let mut changed = false;
        let mut copy = false;
        let mut cut = false;
        if ui.input_mut(|input| input.consume_key(command, Key::C)) {
            copy = true;
        }
        if ui.input_mut(|input| input.consume_key(command, Key::X)) {
            cut = true;
        }
        let shift = ui.input(|input| input.modifiers.shift);
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Backspace)) {
            self.snapshot();
            if self.selection.is_some() {
                self.delete_selection();
            } else {
                edit::backspace(&mut self.formula, &mut self.caret);
            }
            changed = true;
        }
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Delete)) {
            self.snapshot();
            if self.selection.is_some() {
                self.delete_selection();
            } else {
                edit::delete_forward(&mut self.formula, &mut self.caret);
            }
            changed = true;
        }
        self.arrow(ui, Key::ArrowLeft, shift, |formula, caret| {
            edit::move_left(formula, caret);
        });
        self.arrow(ui, Key::ArrowRight, shift, |formula, caret| {
            edit::move_right(formula, caret);
        });
        self.arrow(ui, Key::ArrowUp, shift, |formula, caret| {
            edit::move_vertical(formula, caret, true);
        });
        self.arrow(ui, Key::ArrowDown, shift, |formula, caret| {
            edit::move_vertical(formula, caret, false);
        });
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Home)) {
            self.caret.index = 0;
            self.finish_move(shift);
        }
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::End)) {
            let len = self
                .formula
                .row(&self.caret.trail)
                .map_or(0, |row| row.nodes.len());
            self.caret.index = len;
            self.finish_move(shift);
        }
        changed | self.clipboard(ui, copy, cut)
    }

    fn clipboard(&mut self, ui: &mut egui::Ui, mut copy: bool, mut cut: bool) -> bool {
        let mut pasted = None;
        let mut typed = String::new();
        ui.input_mut(|input| {
            input.events.retain(|event| match event {
                egui::Event::Copy => {
                    copy = true;
                    false
                }
                egui::Event::Cut => {
                    cut = true;
                    false
                }
                egui::Event::Paste(text) => {
                    pasted = Some(text.clone());
                    false
                }
                egui::Event::Text(text) => {
                    typed.push_str(text);
                    false
                }
                _ => true,
            });
        });
        let mut changed = false;
        if copy || cut {
            ui.ctx().copy_text(self.copied_mathml());
        }
        if cut && self.selection.is_some() {
            self.snapshot();
            self.delete_selection();
            changed = true;
        }
        if let Some(text) = pasted {
            if let Ok(nodes) = mathml::nodes_from_mathml(&text) {
                self.snapshot();
                self.delete_selection();
                edit::insert_nodes(&mut self.formula, &mut self.caret, nodes);
                self.anchor.clone_from(&self.caret);
                changed = true;
            }
        }
        if !typed.is_empty() {
            self.snapshot();
            self.delete_selection();
            changed |= insert_str(&mut self.formula, &mut self.caret, &typed);
        }
        changed
    }

    fn arrow(
        &mut self,
        ui: &mut egui::Ui,
        key: Key,
        shift: bool,
        step: impl FnOnce(&Formula, &mut Caret),
    ) {
        let modifiers = if shift {
            Modifiers::SHIFT
        } else {
            Modifiers::NONE
        };
        if ui.input_mut(|input| input.consume_key(modifiers, key)) {
            if shift && self.selection.is_none() {
                self.anchor.clone_from(&self.caret);
            }
            step(&self.formula, &mut self.caret);
            self.finish_move(shift);
        }
    }

    fn finish_move(&mut self, shift: bool) {
        if shift {
            self.selection = edit::selection_between(&self.anchor, &self.caret);
        } else {
            self.anchor.clone_from(&self.caret);
            self.selection = None;
        }
    }

    fn copied_mathml(&self) -> String {
        let Some(selection) = &self.selection else {
            return mathml::write(&self.formula);
        };
        let Some(row) = self.formula.row(&selection.trail) else {
            return mathml::write(&self.formula);
        };
        let end = selection.end.min(row.nodes.len());
        let start = selection.start.min(end);
        if start == end {
            return mathml::write(&self.formula);
        }
        mathml::write_nodes(&row.nodes[start..end])
    }

    fn source_panel(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.add_space(8.0);
        ui.label(RichText::new("MathML").strong());
        let edit = TextEdit::multiline(&mut self.source)
            .font(egui::TextStyle::Monospace)
            .desired_rows(6)
            .desired_width(f32::INFINITY);
        if ui.add(edit).changed() {
            self.source_edited = true;
        }
        ui.horizontal(|ui| {
            let button = egui::Button::new(RichText::new(&self.text.apply).color(Color32::WHITE))
                .fill(ui.visuals().selection.bg_fill);
            if ui.add_enabled(self.source_edited, button).clicked() {
                match mathml::parse(&self.source) {
                    Ok(formula) => {
                        self.snapshot();
                        self.formula = formula;
                        self.caret = Caret::default();
                        self.anchor = Caret::default();
                        self.selection = None;
                        self.parse_error = None;
                        self.source_edited = false;
                        changed = true;
                    }
                    Err(error) => self.parse_error = Some(error.to_string()),
                }
            }
            if let Some(error) = &self.parse_error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        });
        changed
    }
}

fn paper(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(36, 36, 40)
    } else {
        Color32::WHITE
    }
}

enum Action {
    Undo,
    Redo,
    Fraction,
    Sqrt,
    Root,
    Sup,
    Sub,
    Fenced {
        open: &'static str,
        close: &'static str,
    },
    Matrix {
        rows: usize,
        columns: usize,
        open: &'static str,
        close: &'static str,
    },
    Sum,
    Integral,
    Product,
    Over(&'static str),
    Lim,
    Func(&'static str),
    Symbol(String),
}

fn tool_row(ui: &mut egui::Ui, text: &Text) -> Option<Action> {
    let mut action = None;
    let take = |ui: &mut egui::Ui, icon: Icon, tip: &str, made: Action| {
        if icon_button(ui, icon, tip).clicked() {
            Some(made)
        } else {
            None
        }
    };
    action = take(ui, Icon::Undo, &text.undo, Action::Undo).or(action);
    action = take(ui, Icon::Redo, &text.redo, Action::Redo).or(action);
    ui.separator();
    action = take(ui, Icon::Sup, &text.superscript, Action::Sup).or(action);
    action = take(ui, Icon::Sub, &text.subscript, Action::Sub).or(action);
    action = take(ui, Icon::Fraction, &text.fraction, Action::Fraction).or(action);
    action = take(ui, Icon::Sqrt, &text.square_root, Action::Sqrt).or(action);
    action = take(ui, Icon::Root, &text.nth_root, Action::Root).or(action);
    action = take(ui, Icon::Sum, &text.sum, Action::Sum).or(action);
    action = take(ui, Icon::Integral, &text.integral, Action::Integral).or(action);
    action = take(ui, Icon::Product, &text.product, Action::Product).or(action);
    action = bracket_menu(ui, text).or(action);
    action = matrix_menu(ui, text).or(action);
    action = accent_menu(ui, text).or(action);
    action = take(ui, Icon::Label("lim"), &text.limit, Action::Lim).or(action);
    action = take(ui, Icon::Label("sin"), &text.sine, Action::Func("sin")).or(action);
    action = take(ui, Icon::Label("cos"), &text.cosine, Action::Func("cos")).or(action);
    symbol_menu(ui, text, &mut action);
    action
}

fn icon_button(ui: &mut egui::Ui, icon: Icon, tip: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(48.0, 40.0), Sense::click());
    paint_tool_frame(ui, rect, &response);
    let color = ui.style().interact(&response).fg_stroke.color;
    icons::paint(ui.painter(), rect.shrink(8.0), icon, color);
    response.on_hover_text(tip)
}

fn paint_tool_frame(ui: &egui::Ui, rect: egui::Rect, response: &egui::Response) {
    let visuals = ui.style().interact(response);
    if response.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(5),
            ui.visuals().widgets.hovered.weak_bg_fill,
        );
    }
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(5),
        visuals.bg_stroke,
        egui::StrokeKind::Inside,
    );
}

fn bracket_menu(ui: &mut egui::Ui, text: &Text) -> Option<Action> {
    let response = icon_button(ui, Icon::Parens, &text.parentheses);
    let mut chosen = None;
    Popup::menu(&response)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            chosen = pair_row(ui);
        });
    chosen
}

fn pair_row(ui: &mut egui::Ui) -> Option<Action> {
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
        for (open, close, kind) in PAIRS {
            if pair_button(ui, kind).clicked() {
                chosen = Some(Action::Fenced { open, close });
                ui.close();
            }
        }
    });
    chosen
}

fn pair_button(ui: &mut egui::Ui, kind: Bracket) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(36.0, 36.0), Sense::click());
    let fill = if response.hovered() {
        ui.visuals().widgets.hovered.weak_bg_fill
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
    icons::brackets(
        ui.painter(),
        rect.shrink(5.0),
        kind,
        ui.visuals().text_color(),
    );
    response
}

fn accent_menu(ui: &mut egui::Ui, text: &Text) -> Option<Action> {
    let response = icon_button(ui, Icon::Bar, &text.overline);
    let mut chosen = None;
    Popup::menu(&response)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            chosen = accent_row(ui);
        });
    chosen
}

fn accent_row(ui: &mut egui::Ui) -> Option<Action> {
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
        for (symbol, kind) in ACCENTS {
            if accent_button(ui, kind).clicked() {
                chosen = Some(Action::Over(symbol));
                ui.close();
            }
        }
    });
    chosen
}

fn accent_button(ui: &mut egui::Ui, kind: Accent) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(36.0, 36.0), Sense::click());
    let fill = if response.hovered() {
        ui.visuals().widgets.hovered.weak_bg_fill
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
    icons::accent(
        ui.painter(),
        rect.shrink(4.0),
        kind,
        ui.visuals().text_color(),
    );
    response
}

fn matrix_menu(ui: &mut egui::Ui, text: &Text) -> Option<Action> {
    let response = icon_button(ui, Icon::Matrix, &text.matrix);
    let fence_id = response.id.with("fence");
    Popup::menu(&response)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| matrix_popup(ui, fence_id, text))
        .and_then(|inner| inner.inner)
}

fn matrix_popup(ui: &mut egui::Ui, fence_id: Id, text: &Text) -> Option<Action> {
    ui.set_min_width(220.0);
    let fence = fence_row(ui, fence_id);
    ui.add_space(6.0);
    let (rows, columns, clicked) = size_grid(ui);
    ui.add_space(4.0);
    if rows == 0 {
        ui.label(&text.pick_cells);
    } else {
        ui.label(format!("{rows}×{columns}"));
    }
    if !clicked {
        return None;
    }
    ui.close();
    let (open, close) = FENCES[fence];
    Some(Action::Matrix {
        rows,
        columns,
        open,
        close,
    })
}

fn fence_row(ui: &mut egui::Ui, fence_id: Id) -> usize {
    let mut fence = ui
        .data(|data| data.get_temp::<usize>(fence_id))
        .unwrap_or(1)
        .min(FENCES.len() - 1);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
        for (index, kind) in FENCE_KINDS.iter().enumerate() {
            let (rect, response) = ui.allocate_exact_size(Vec2::new(32.0, 32.0), Sense::click());
            if response.clicked() {
                fence = index;
                ui.data_mut(|data| data.insert_temp(fence_id, index));
            }
            let fill = if fence == index {
                ui.visuals().selection.bg_fill.gamma_multiply(0.35)
            } else if response.hovered() {
                ui.visuals().widgets.hovered.weak_bg_fill
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
            icons::brackets(
                ui.painter(),
                rect.shrink(5.0),
                *kind,
                ui.visuals().text_color(),
            );
        }
    });
    fence
}

fn size_grid(ui: &mut egui::Ui) -> (usize, usize, bool) {
    const N: usize = 8;
    let mut cells = Vec::with_capacity(N * N);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        for row in 0..N {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                for column in 0..N {
                    let (rect, response) =
                        ui.allocate_exact_size(Vec2::new(18.0, 18.0), Sense::click());
                    cells.push((row, column, rect, response));
                }
            });
        }
    });
    let hot = cells
        .iter()
        .find(|(_, _, _, response)| response.hovered())
        .map(|(row, column, _, _)| (*row, *column));
    let accent = ui.visuals().selection.bg_fill;
    let idle = ui.visuals().widgets.inactive.bg_fill;
    for (row, column, rect, _) in &cells {
        let on = hot.is_some_and(|(hot_row, hot_column)| *row <= hot_row && *column <= hot_column);
        ui.painter().rect_filled(
            *rect,
            2.0,
            if on { accent.gamma_multiply(0.8) } else { idle },
        );
    }
    let clicked = cells
        .iter()
        .find(|(_, _, _, response)| response.clicked())
        .map(|(row, column, _, _)| (*row + 1, *column + 1));
    match (clicked, hot) {
        (Some((rows, columns)), _) => (rows, columns, true),
        (None, Some((row, column))) => (row + 1, column + 1, false),
        (None, None) => (0, 0, false),
    }
}

fn symbol_menu(ui: &mut egui::Ui, text: &Text, action: &mut Option<Action>) {
    let response = icon_button(ui, Icon::Palette, &text.symbols);
    Popup::menu(&response)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(280.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                for symbol in SYMBOLS {
                    if symbol_button(ui, symbol).clicked() {
                        *action = Some(Action::Symbol((*symbol).to_owned()));
                        ui.close();
                    }
                }
            });
        });
}

fn symbol_button(ui: &mut egui::Ui, symbol: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(32.0, 32.0), Sense::click());
    paint_tool_frame(ui, rect, &response);
    let color = ui.visuals().text_color();
    let font = FontId::proportional(18.0);
    let missing = ui.ctx().fonts_mut(|fonts| !fonts.has_glyphs(&font, symbol));
    if missing {
        icons::symbol(ui.painter(), rect.shrink(5.0), symbol, color);
    } else {
        ui.painter()
            .text(rect.center(), Align2::CENTER_CENTER, symbol, font, color);
    }
    response
}

const FENCE_KINDS: [Bracket; 9] = [
    Bracket::None,
    Bracket::Round,
    Bracket::Square,
    Bracket::Curly,
    Bracket::Single,
    Bracket::Double,
    Bracket::Angle,
    Bracket::Ceil,
    Bracket::Floor,
];

const FENCES: [(&str, &str); 9] = [
    ("", ""),
    ("(", ")"),
    ("[", "]"),
    ("{", "}"),
    ("|", "|"),
    ("‖", "‖"),
    ("⟨", "⟩"),
    ("⌈", "⌉"),
    ("⌊", "⌋"),
];

const PAIRS: [(&str, &str, Bracket); 8] = [
    ("(", ")", Bracket::Round),
    ("[", "]", Bracket::Square),
    ("{", "}", Bracket::Curly),
    ("|", "|", Bracket::Single),
    ("‖", "‖", Bracket::Double),
    ("⟨", "⟩", Bracket::Angle),
    ("⌈", "⌉", Bracket::Ceil),
    ("⌊", "⌋", Bracket::Floor),
];

const ACCENTS: [(&str, Accent); 8] = [
    ("‾", Accent::Bar),
    ("ˆ", Accent::Hat),
    ("˜", Accent::Tilde),
    ("˙", Accent::Dot),
    ("¨", Accent::Ddot),
    ("→", Accent::Arrow),
    ("ˇ", Accent::Check),
    ("˘", Accent::Breve),
];

const SYMBOLS: &[&str] = &[
    "α", "β", "γ", "δ", "ε", "θ", "λ", "μ", "π", "σ", "φ", "ω", "Δ", "Σ", "Ω", "≤", "≥", "≠", "≈",
    "±", "×", "·", "÷", "∞", "∂", "→", "∈", "∉", "⊂",
];

#[cfg(test)]
mod tests {
    use super::MathEditor;

    #[test]
    fn the_widget_lays_out_a_formula() {
        let ctx = egui::Context::default();
        let mut editor = MathEditor::from_mathml(
            "<math xmlns=\"http://www.w3.org/1998/Math/MathML\"><mfrac><mi>a</mi><mi>b</mi></mfrac></math>",
        )
        .unwrap();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 600.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            assert!(!editor.show(ui));
        });
        assert!(editor.mathml().contains("<mfrac>"));
    }
}
