//! The egui widget: a mode toolbar, a zoomable canvas and an inspector.
//!
//! Drawing is backend-independent: [`crate::render`] turns each object into
//! primitives and this module only maps them to screen coordinates.

#![allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::many_single_char_names
)]

use egui::{
    Align, Align2, Color32, CornerRadius, DragValue, FontId, Frame, Key, Layout, Margin, Modifiers,
    Popup, PopupCloseBehavior, Pos2, Rect, RichText, ScrollArea, Sense, Shape, Stroke, StrokeKind,
    TextEdit, Vec2,
};

use crate::editor::{AlignEdge, Axis, ChemEditor, Drag, Mode, Target, Tool};
use crate::element::{self, ELEMENTS};
use crate::icons::{self, Icon};
use crate::model::{ArrowKind, BondOrder, BondStereo, ChemObject, Point};
use crate::render::{object_prims, prims_bounds, Align as TextAlign, Prim, Theme};

const INSPECTOR_WIDTH: f32 = 292.0;
const ATOM_HIT: f32 = 15.0;
const BOND_HIT: f32 = 7.0;

/// Maps world (document) coordinates to canvas screen coordinates.
#[derive(Clone, Copy)]
struct View {
    rect: Rect,
    pan: Vec2,
    zoom: f32,
}

impl View {
    fn screen(self, world: Point) -> Pos2 {
        self.rect.min + self.pan + world.scale(self.zoom).to_pos2()
    }

    fn world(self, screen: Pos2) -> Point {
        Point::new(
            (screen.x - self.rect.min.x - self.pan.x) / self.zoom,
            (screen.y - self.rect.min.y - self.pan.y) / self.zoom,
        )
    }
}

impl Point {
    fn to_pos2(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

fn color(value: [u8; 3]) -> Color32 {
    Color32::from_rgb(value[0], value[1], value[2])
}

fn paper(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(38, 38, 42)
    } else {
        Color32::WHITE
    }
}

impl ChemEditor {
    /// Draws the whole editor. Returns `true` when the document changed.
    pub fn show(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = self.toolbar(ui);
        ui.add_space(4.0);
        let size = ui.available_size();
        let inspector = INSPECTOR_WIDTH.min((size.x * 0.45).max(200.0));
        ui.horizontal_top(|ui| {
            let canvas = Vec2::new((size.x - inspector - 12.0).max(160.0), size.y);
            ui.allocate_ui_with_layout(canvas, Layout::top_down(Align::Min), |ui| {
                changed |= self.canvas(ui);
            });
            ui.separator();
            ui.allocate_ui_with_layout(
                Vec2::new(inspector, size.y),
                Layout::top_down(Align::Min),
                |ui| {
                    ScrollArea::vertical().show(ui, |ui| {
                        changed |= self.inspector(ui);
                    });
                },
            );
        });
        changed
    }

    // ----- toolbar --------------------------------------------------------

    fn toolbar(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        Frame::new()
            .fill(ui.visuals().panel_fill)
            .inner_margin(Margin::same(6))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().button_padding = Vec2::new(6.0, 3.0);
                    ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                    changed |= self.mode_row(ui);
                    ui.separator();
                    changed |= self.tool_row(ui);
                    ui.separator();
                    changed |= self.palette_row(ui);
                    ui.separator();
                    changed |= self.view_row(ui);
                });
            });
        changed
    }

    fn mode_row(&mut self, ui: &mut egui::Ui) -> bool {
        let locale = self.locale.clone();
        let mut changed = false;
        for (mode, key) in [(Mode::Structure, "Structure"), (Mode::Reaction, "Reaction")] {
            let selected = self.mode == mode;
            if ui
                .selectable_label(selected, RichText::new(locale.text(key)).strong())
                .clicked()
                && !selected
            {
                self.set_mode(mode);
                changed = true;
            }
        }
        changed
    }

    fn tool_row(&mut self, ui: &mut egui::Ui) -> bool {
        let locale = self.locale.clone();
        let mut changed = false;
        let tools: [(&str, Icon, Tool); 6] = [
            ("Select", Icon::Select, Tool::Select),
            ("Move", Icon::Move, Tool::Move),
            ("Atom", Icon::Atom, Tool::Atom(self.element)),
            ("Bond", Icon::Bond, Tool::Bond),
            ("Ring", Icon::Ring, Tool::Ring),
            ("Erase", Icon::Erase, Tool::Erase),
        ];
        for (key, icon, tool) in tools {
            let selected = std::mem::discriminant(&self.tool) == std::mem::discriminant(&tool);
            if icon_toggle(ui, icon, locale.text(key), selected).clicked() {
                self.tool = tool;
                changed = true;
            }
        }
        changed
    }

    fn palette_row(&mut self, ui: &mut egui::Ui) -> bool {
        let locale = self.locale.clone();
        let mut changed = false;
        if self.mode == Mode::Structure {
            let symbol = element::symbol(self.element);
            let response = element_button(ui, self.element, &locale);
            if let Some(picked) = element_popup(&response, locale.text("Elements"), &locale) {
                self.set_element(picked);
                changed = true;
            }
            ui.label(RichText::new(symbol).strong());
            egui::ComboBox::from_id_salt("chem-bond-order")
                .selected_text(locale.text(bond_order_key(self.bond_order)))
                .width(96.0)
                .show_ui(ui, |ui| {
                    for order in [
                        BondOrder::Single,
                        BondOrder::Double,
                        BondOrder::Triple,
                        BondOrder::Aromatic,
                    ] {
                        if ui
                            .selectable_label(
                                self.bond_order == order,
                                locale.text(bond_order_key(order)),
                            )
                            .clicked()
                        {
                            self.set_bond_style(order, self.bond_stereo);
                            changed = true;
                        }
                    }
                });
            egui::ComboBox::from_id_salt("chem-bond-stereo")
                .selected_text(locale.text(stereo_key(self.bond_stereo)))
                .width(88.0)
                .show_ui(ui, |ui| {
                    for stereo in [BondStereo::Plain, BondStereo::Wedge, BondStereo::Hash] {
                        if ui
                            .selectable_label(
                                self.bond_stereo == stereo,
                                locale.text(stereo_key(stereo)),
                            )
                            .clicked()
                        {
                            self.set_bond_style(self.bond_order, stereo);
                            changed = true;
                        }
                    }
                });
            egui::ComboBox::from_id_salt("chem-ring")
                .selected_text(format!("{}", self.ring_size))
                .width(52.0)
                .show_ui(ui, |ui| {
                    for size in 3..=8 {
                        if ui
                            .selectable_label(self.ring_size == size, format!("{size}"))
                            .clicked()
                        {
                            self.set_ring_size(size);
                            changed = true;
                        }
                    }
                });
        }
        changed
    }

    fn view_row(&mut self, ui: &mut egui::Ui) -> bool {
        let locale = self.locale.clone();
        let mut changed = false;
        if icon_button(ui, Icon::Undo, locale.text("Undo")).clicked() {
            self.undo();
            changed = true;
        }
        if icon_button(ui, Icon::Redo, locale.text("Redo")).clicked() {
            self.redo();
            changed = true;
        }
        if icon_button(ui, Icon::ZoomIn, locale.text("Zoom in")).clicked() {
            self.zoom = (self.zoom * 1.2).clamp(0.2, 8.0);
        }
        if icon_button(ui, Icon::ZoomOut, locale.text("Zoom out")).clicked() {
            self.zoom = (self.zoom / 1.2).clamp(0.2, 8.0);
        }
        if icon_button(ui, Icon::Reset, locale.text("Reset view")).clicked() {
            self.zoom = 1.0;
            self.pan = egui::vec2(60.0, 40.0);
        }
        if icon_button(ui, Icon::Fit, locale.text("Fit to window")).clicked() {
            self.request_fit();
        }
        let mut snap = self.snap;
        if ui.checkbox(&mut snap, locale.text("Snap")).changed() {
            self.set_snap(snap);
        }
        let mut show_carbon = self.show_carbon;
        if ui
            .checkbox(&mut show_carbon, locale.text("Show carbons"))
            .changed()
        {
            self.set_show_carbon(show_carbon);
        }
        let mut colored = self.colored_atoms;
        if ui
            .checkbox(&mut colored, locale.text("Colored atoms"))
            .changed()
        {
            self.set_colored_atoms(colored);
        }
        changed
    }

    // ----- canvas ---------------------------------------------------------

    fn canvas(&mut self, ui: &mut egui::Ui) -> bool {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        if self.want_fit {
            self.want_fit = false;
            self.fit(rect);
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::same(4), paper(ui));
        painter.rect_stroke(
            rect,
            CornerRadius::same(4),
            Stroke::new(1.0, ui.visuals().widgets.inactive.bg_stroke.color),
            StrokeKind::Inside,
        );
        let view = View {
            rect,
            pan: self.pan,
            zoom: self.zoom,
        };
        let hovered = response
            .hover_pos()
            .map(|point| self.hit_test(view.world(point)));
        let mut changed = self.paint_scene(ui, &painter, view);
        self.paint_overlay(&painter, view, hovered.flatten());
        changed |= self.interact(ui, &response, view);
        self.shortcuts(ui);
        response.on_hover_cursor(match hovered.flatten() {
            Some(Target::Atom(..) | Target::Bond(..)) => egui::CursorIcon::PointingHand,
            Some(Target::Object(_)) => egui::CursorIcon::Grab,
            None => egui::CursorIcon::Crosshair,
        });
        changed
    }

    fn fit(&mut self, rect: Rect) {
        let mut min = Point::new(f32::INFINITY, f32::INFINITY);
        let mut max = Point::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
        let mut any = false;
        for object in &self.document.objects {
            if let Some((a, b)) = object_bounds(object) {
                min.x = min.x.min(a.x);
                min.y = min.y.min(a.y);
                max.x = max.x.max(b.x);
                max.y = max.y.max(b.y);
                any = true;
            }
        }
        if !any {
            self.zoom = 1.0;
            self.pan = egui::vec2(40.0, 40.0);
            return;
        }
        let size = Point::new((max.x - min.x).max(1.0), (max.y - min.y).max(1.0));
        let margin = 64.0;
        let zoom = ((rect.width() - margin) / size.x)
            .min((rect.height() - margin) / size.y)
            .clamp(0.2, 8.0);
        let center = Point::new(min.x.midpoint(max.x), min.y.midpoint(max.y));
        self.zoom = zoom;
        self.pan = egui::vec2(
            rect.width() / 2.0 - center.x * zoom,
            rect.height() / 2.0 - center.y * zoom,
        );
    }

    fn theme(&self, ui: &egui::Ui) -> Theme {
        let ink = ui.visuals().text_color().to_array();
        Theme {
            ink: [ink[0], ink[1], ink[2]],
            font: 18.0,
            colored_atoms: self.colored_atoms,
        }
    }

    fn paint_scene(&self, ui: &egui::Ui, painter: &egui::Painter, view: View) -> bool {
        let theme = self.theme(ui);
        for object in &self.document.objects {
            let prims = object_prims(object, &theme, self.show_carbon);
            for prim in &prims {
                paint_prim(painter, view, prim);
            }
        }
        false
    }

    fn paint_overlay(&self, painter: &egui::Painter, view: View, hovered: Option<Target>) {
        let accent = Color32::from_rgb(43, 87, 154);
        if let Some(id) = self.active {
            if let Some(object) = self.document.object(id) {
                if let Some((min, max)) = object_bounds(object) {
                    let rect = Rect::from_min_max(
                        view.screen(Point::new(min.x - 12.0, min.y - 12.0)),
                        view.screen(Point::new(max.x + 12.0, max.y + 12.0)),
                    );
                    painter.rect_stroke(
                        rect,
                        CornerRadius::same(4),
                        Stroke::new(1.0, accent.gamma_multiply(0.35)),
                        StrokeKind::Outside,
                    );
                }
            }
        }
        if let Some(target) = hovered {
            if !self.is_selected(target) {
                highlight_target(
                    painter,
                    view,
                    self,
                    target,
                    accent.gamma_multiply(0.45),
                    1.5,
                );
            }
        }
        for target in self.selected() {
            highlight_target(painter, view, self, target, accent, 2.5);
        }
        match self.drag {
            Some(Drag::Marquee { start, current, .. }) => {
                let rect = Rect::from_two_pos(view.screen(start), view.screen(current));
                painter.rect_filled(rect, 2.0, accent.gamma_multiply(0.12));
                painter.rect_stroke(rect, 2.0, Stroke::new(1.0, accent), StrokeKind::Inside);
            }
            Some(Drag::Bond {
                structure,
                from,
                current,
            }) => {
                if let Some(a) = self.atom_world(structure, from) {
                    painter.line_segment(
                        [view.screen(a), view.screen(current)],
                        Stroke::new(1.8 * view.zoom, accent),
                    );
                }
            }
            _ => {}
        }
    }

    fn interact(&mut self, ui: &mut egui::Ui, response: &egui::Response, view: View) -> bool {
        let mut changed = false;
        self.scroll(ui, response, view);
        if response.dragged_by(egui::PointerButton::Middle) {
            self.pan += response.drag_delta();
            return false;
        }
        let Some(pointer) = response.interact_pointer_pos() else {
            return false;
        };
        let world = view.world(pointer);
        let shift = ui.input(|input| input.modifiers.shift);
        if response.drag_started() {
            self.drag_start(world, pointer, shift);
        } else if response.dragged() {
            changed |= self.drag_move(world, pointer);
        } else if response.drag_stopped() {
            changed |= self.drag_finish(world);
        } else if response.clicked() {
            changed |= self.click(world, shift);
        }
        changed
    }

    fn scroll(&mut self, ui: &egui::Ui, response: &egui::Response, view: View) {
        let (scroll, zoom_key) =
            ui.input(|input| (input.smooth_scroll_delta, input.modifiers.command));
        if scroll != Vec2::ZERO {
            if zoom_key {
                if let Some(pointer) = response.hover_pos() {
                    let before = view.world(pointer);
                    let factor = (scroll.y * 0.002).exp();
                    self.zoom = (self.zoom * factor).clamp(0.2, 8.0);
                    self.pan = pointer - view.rect.min - before.scale(self.zoom).to_pos2();
                }
            } else {
                self.pan += scroll;
            }
        }
    }

    fn drag_start(&mut self, world: Point, pointer: Pos2, shift: bool) {
        match self.tool {
            Tool::Bond => {
                if let Some(Target::Atom(structure, atom)) = self.hit_test(world) {
                    self.drag = Some(Drag::Bond {
                        structure,
                        from: atom,
                        current: world,
                    });
                }
            }
            Tool::Move => {
                if let Some(id) = self.hit_test(world).map(owning_object) {
                    self.active = Some(id);
                    self.select(Some(Target::Object(id))).ok();
                    self.begin();
                    self.drag = Some(Drag::Move { last: world });
                } else {
                    self.drag = Some(Drag::Pan { last: pointer });
                }
            }
            Tool::Select => match self.hit_test(world) {
                Some(Target::Atom(structure, atom)) => {
                    let target = Target::Atom(structure, atom);
                    if !self.is_selected(target) {
                        if shift {
                            self.toggle(target).ok();
                        } else {
                            self.select(Some(target)).ok();
                        }
                    }
                    self.active = Some(structure);
                    self.begin();
                    self.drag = Some(Drag::Move { last: world });
                }
                Some(Target::Bond(structure, _)) => {
                    self.active = Some(structure);
                    self.select(Some(Target::Object(structure))).ok();
                    self.begin();
                    self.drag = Some(Drag::Move { last: world });
                }
                Some(Target::Object(id)) => {
                    self.active = Some(id);
                    self.select(Some(Target::Object(id))).ok();
                    self.begin();
                    self.drag = Some(Drag::Move { last: world });
                }
                None => {
                    if !shift {
                        self.clear_selection();
                    }
                    self.drag = Some(Drag::Marquee {
                        start: world,
                        current: world,
                        additive: shift,
                    });
                }
            },
            _ => {
                self.drag = Some(Drag::Pan { last: pointer });
            }
        }
    }

    fn drag_move(&mut self, world: Point, pointer: Pos2) -> bool {
        match self.drag {
            Some(Drag::Pan { last }) => {
                self.pan += pointer - last;
                self.drag = Some(Drag::Pan { last: pointer });
                false
            }
            Some(Drag::Move { last }) => {
                let delta = world.sub(last);
                self.drag = Some(Drag::Move { last: world });
                self.drag_selection(delta);
                true
            }
            Some(Drag::Bond {
                structure, from, ..
            }) => {
                self.drag = Some(Drag::Bond {
                    structure,
                    from,
                    current: world,
                });
                false
            }
            Some(Drag::Marquee {
                start, additive, ..
            }) => {
                let mut targets = if additive { self.selected() } else { vec![] };
                targets.extend(self.targets_in_rect(start, world));
                self.set_selection(targets).ok();
                self.drag = Some(Drag::Marquee {
                    start,
                    current: world,
                    additive,
                });
                false
            }
            None => false,
        }
    }

    fn drag_finish(&mut self, world: Point) -> bool {
        let drag = self.drag.take();
        self.commit();
        if let Some(Drag::Bond {
            structure, from, ..
        }) = drag
        {
            if let Some(Target::Atom(_, atom)) = self.hit_test(world) {
                if atom != from {
                    return self.connect(from, atom).is_ok();
                }
                return false;
            }
            self.active = Some(structure);
            return self.bond_from(from, self.element, world).is_ok();
        }
        false
    }

    fn targets_in_rect(&self, start: Point, current: Point) -> Vec<Target> {
        let min = Point::new(start.x.min(current.x), start.y.min(current.y));
        let max = Point::new(start.x.max(current.x), start.y.max(current.y));
        let inside = |point: Point| {
            point.x >= min.x && point.x <= max.x && point.y >= min.y && point.y <= max.y
        };
        let mut targets = vec![];
        for object in &self.document.objects {
            match object {
                ChemObject::Structure(structure) => {
                    let (bounds_min, bounds_max) = structure.bounds();
                    if inside(bounds_min) && inside(bounds_max) {
                        targets.push(Target::Object(structure.id));
                        continue;
                    }
                    for atom in &structure.atoms {
                        if inside(atom.position.add(structure.origin)) {
                            targets.push(Target::Atom(structure.id, atom.id));
                        }
                    }
                    for bond in &structure.bonds {
                        let (Some(a), Some(b)) = (
                            structure
                                .atom(bond.a)
                                .map(|x| x.position.add(structure.origin)),
                            structure
                                .atom(bond.b)
                                .map(|x| x.position.add(structure.origin)),
                        ) else {
                            continue;
                        };
                        if inside(a) && inside(b) {
                            targets.push(Target::Bond(structure.id, bond.id));
                        }
                    }
                }
                ChemObject::Reaction(_) => {
                    if let Some((bounds_min, bounds_max)) = object_bounds(object) {
                        if inside(bounds_min) && inside(bounds_max) {
                            targets.push(Target::Object(object.id()));
                        }
                    }
                }
            }
        }
        targets
    }

    fn select_all(&mut self) {
        let mut targets = vec![];
        for object in &self.document.objects {
            match object {
                ChemObject::Structure(structure) => {
                    targets.extend(
                        structure
                            .atoms
                            .iter()
                            .map(|atom| Target::Atom(structure.id, atom.id)),
                    );
                    targets.extend(
                        structure
                            .bonds
                            .iter()
                            .map(|bond| Target::Bond(structure.id, bond.id)),
                    );
                }
                ChemObject::Reaction(_) => targets.push(Target::Object(object.id())),
            }
        }
        self.set_selection(targets).ok();
    }

    fn click(&mut self, world: Point, shift: bool) -> bool {
        match self.tool {
            Tool::Atom(element) => self.add_atom(element, world).is_ok(),
            Tool::Ring => self.add_ring(world, self.ring_size).is_ok(),
            Tool::Erase => {
                if let Some(target) = self.hit_test(world) {
                    self.select(Some(target)).ok();
                    self.delete_selected();
                    true
                } else {
                    false
                }
            }
            Tool::Select => {
                let hit = self.hit_test(world);
                if let Some(target) = hit {
                    self.active = Some(owning_object(target));
                }
                if shift {
                    if let Some(target) = hit {
                        self.toggle(target).ok();
                    }
                } else {
                    self.select(hit).ok();
                }
                false
            }
            Tool::Move => {
                if let Some(target) = self.hit_test(world) {
                    let id = owning_object(target);
                    self.active = Some(id);
                    self.select(Some(Target::Object(id))).ok();
                }
                false
            }
            Tool::Bond => false,
        }
    }

    fn shortcuts(&mut self, ui: &mut egui::Ui) {
        if ui.memory(|memory| memory.focused().is_some()) {
            return;
        }
        let mut paste = None;
        ui.input_mut(|input| {
            input.events.retain(|event| {
                if let egui::Event::Paste(text) = event {
                    paste = Some(text.clone());
                    false
                } else {
                    true
                }
            });
        });
        if let Some(text) = paste {
            self.paste_json(&text).ok();
            return;
        }
        if ui.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::C)) {
            if let Some(json) = self.copy_selection() {
                ui.ctx().copy_text(json);
            }
            return;
        }
        if ui.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::X)) {
            if let Some(json) = self.cut_selection() {
                ui.ctx().copy_text(json);
            }
            return;
        }
        if ui.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::V)) {
            self.paste_clipboard().ok();
            return;
        }
        if ui.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::Z)) {
            self.undo();
            return;
        }
        if ui.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::Y))
            || ui
                .input_mut(|input| input.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z))
        {
            self.redo();
            return;
        }
        if ui.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::A)) {
            self.select_all();
            return;
        }
        if ui.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::D)) {
            self.duplicate_selected().ok();
            return;
        }
        if ui.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::Num0)) {
            self.request_fit();
            return;
        }
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Delete))
            || ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Backspace))
        {
            self.delete_selected();
            return;
        }
        let step = if ui.input(|input| input.modifiers.shift) {
            12.0
        } else {
            2.0
        };
        for (key, delta) in [
            (Key::ArrowLeft, Point::new(-step, 0.0)),
            (Key::ArrowRight, Point::new(step, 0.0)),
            (Key::ArrowUp, Point::new(0.0, -step)),
            (Key::ArrowDown, Point::new(0.0, step)),
        ] {
            if ui.input_mut(|input| input.consume_key(Modifiers::NONE, key))
                || ui.input_mut(|input| input.consume_key(Modifiers::SHIFT, key))
            {
                self.nudge_selected(delta).ok();
                return;
            }
        }
    }

    fn hit_test(&self, world: Point) -> Option<Target> {
        for object in self.document.objects.iter().rev() {
            if let ChemObject::Structure(structure) = object {
                for atom in structure.atoms.iter().rev() {
                    if atom.position.add(structure.origin).distance(world) <= ATOM_HIT {
                        return Some(Target::Atom(structure.id, atom.id));
                    }
                }
                for bond in structure.bonds.iter().rev() {
                    let (Some(a), Some(b)) = (
                        structure
                            .atom(bond.a)
                            .map(|x| x.position.add(structure.origin)),
                        structure
                            .atom(bond.b)
                            .map(|x| x.position.add(structure.origin)),
                    ) else {
                        continue;
                    };
                    if distance_to_segment(world, a, b) <= BOND_HIT {
                        return Some(Target::Bond(structure.id, bond.id));
                    }
                }
            } else if let Some((min, max)) = object_bounds(object) {
                if world.x >= min.x - 6.0
                    && world.x <= max.x + 6.0
                    && world.y >= min.y - 6.0
                    && world.y <= max.y + 6.0
                {
                    return Some(Target::Object(object.id()));
                }
            }
        }
        None
    }

    // ----- inspector ------------------------------------------------------

    fn inspector(&mut self, ui: &mut egui::Ui) -> bool {
        let locale = self.locale.clone();
        let mut changed = false;
        ui.add_space(4.0);
        ui.label(RichText::new(locale.text("Properties")).strong());
        ui.separator();
        match self.primary() {
            Some(Target::Atom(structure, atom)) => {
                changed |= self.atom_inspector(ui, structure, atom);
            }
            Some(Target::Bond(structure, bond)) => {
                changed |= self.bond_inspector(ui, structure, bond);
            }
            _ => {}
        }
        if self.selection_count() > 1 {
            ui.label(
                RichText::new(
                    locale.format("{count} selected", &[self.selection_count().to_string()]),
                )
                .weak(),
            );
        }
        let atom_count = self
            .selected()
            .iter()
            .filter(|target| matches!(target, Target::Atom(..)))
            .count();
        if atom_count >= 2 {
            changed |= self.arrange_row(ui);
        }
        if let Some(id) = self.active {
            match self.document.object(id) {
                Some(ChemObject::Structure(structure)) => {
                    changed |= self.structure_inspector(ui, structure.id);
                }
                Some(ChemObject::Reaction(reaction)) => {
                    changed |= self.reaction_inspector(ui, reaction.id);
                }
                None => {}
            }
        }
        ui.add_space(8.0);
        let hint = match self.mode {
            Mode::Structure => {
                "Drag a bond to move the whole structure; drag an atom to move it; drag empty space to select a region. Shift adds to the selection; arrows nudge it."
            }
            Mode::Reaction => "Edit the mhchem source or use the palette.",
        };
        ui.label(
            RichText::new(locale.text(hint))
                .italics()
                .color(ui.visuals().weak_text_color()),
        );
        changed
    }

    fn atom_inspector(&mut self, ui: &mut egui::Ui, structure: u64, atom: u64) -> bool {
        let locale = self.locale.clone();
        let Some(atom_ref) = self
            .structure(structure)
            .and_then(|s| s.atom(atom))
            .cloned()
        else {
            return false;
        };
        let mut changed = false;
        ui.label(RichText::new(locale.text("Atom")).strong());
        ui.horizontal(|ui| {
            ui.label(locale.text("Element"));
            let response = element_button(ui, atom_ref.element, &locale);
            if let Some(picked) = element_popup(&response, locale.text("Elements"), &locale) {
                changed |= self.set_atom_element(structure, atom, picked).is_ok();
            }
            ui.label(RichText::new(locale.text(
                element::element(atom_ref.element).map_or("*", |info| info.name),
            )));
        });
        let mut charge = atom_ref.charge;
        ui.horizontal(|ui| {
            ui.label(locale.text("Charge"));
            if ui
                .add(DragValue::new(&mut charge).range(-6..=6).speed(1.0))
                .changed()
            {
                changed |= self.set_atom_charge(structure, atom, charge).is_ok();
            }
        });
        let mut explicit = atom_ref.hydrogens.is_some();
        if ui
            .checkbox(&mut explicit, locale.text("Hydrogens"))
            .changed()
        {
            let value = if explicit { Some(0) } else { None };
            changed |= self.set_atom_hydrogens(structure, atom, value).is_ok();
        }
        if let Some(mut count) = self
            .structure(structure)
            .and_then(|s| s.atom(atom))
            .and_then(|a| a.hydrogens)
        {
            if ui
                .add(DragValue::new(&mut count).range(0..=12).speed(0.2))
                .changed()
            {
                changed |= self
                    .set_atom_hydrogens(structure, atom, Some(count))
                    .is_ok();
            }
        }
        changed
    }

    fn bond_inspector(&mut self, ui: &mut egui::Ui, structure: u64, bond: u64) -> bool {
        let locale = self.locale.clone();
        let Some(bond_ref) = self
            .structure(structure)
            .and_then(|s| s.bonds.iter().find(|b| b.id == bond))
            .cloned()
        else {
            return false;
        };
        let mut changed = false;
        ui.label(RichText::new(locale.text("Bond")).strong());
        ui.horizontal(|ui| {
            ui.label(locale.text("Bond order"));
            for order in [
                BondOrder::Single,
                BondOrder::Double,
                BondOrder::Triple,
                BondOrder::Aromatic,
            ] {
                if ui
                    .selectable_label(bond_ref.order == order, locale.text(bond_order_key(order)))
                    .clicked()
                {
                    changed |= self
                        .set_bond(structure, bond, order, bond_ref.stereo)
                        .is_ok();
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(locale.text("Stereo"));
            for stereo in [BondStereo::Plain, BondStereo::Wedge, BondStereo::Hash] {
                if ui
                    .selectable_label(bond_ref.stereo == stereo, locale.text(stereo_key(stereo)))
                    .clicked()
                {
                    changed |= self
                        .set_bond(structure, bond, bond_ref.order, stereo)
                        .is_ok();
                }
            }
        });
        changed
    }

    fn structure_inspector(&mut self, ui: &mut egui::Ui, structure: u64) -> bool {
        let locale = self.locale.clone();
        let Some(target) = self.structure(structure) else {
            return false;
        };
        let formula = target.molecular_formula();
        let mass = target.molar_mass();
        let smiles = target.smiles();
        let atoms = target.atoms.len();
        let bonds = target.bonds.len();
        let mut changed = false;
        ui.label(RichText::new(locale.text("Structure")).strong());
        ui.label(format!(
            "{} {}",
            locale.text("Formula"),
            if formula.is_empty() { "–" } else { &formula }
        ));
        ui.label(format!("{} {mass:.2} g/mol", locale.text("Molar mass")));
        ui.horizontal(|ui| {
            ui.label(format!(
                "{} {atoms}   {} {bonds}",
                locale.text("Atom"),
                locale.text("Bond")
            ));
        });
        ui.horizontal(|ui| {
            ui.label(locale.text("SMILES"));
            match &smiles {
                Ok(value) => {
                    ui.monospace(value);
                    if ui.button(locale.text("Copy")).clicked() {
                        ui.ctx().copy_text(value.clone());
                    }
                }
                Err(error) => {
                    ui.colored_label(ui.visuals().warn_fg_color, locale.diagnostic(error));
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button(locale.text("Duplicate")).clicked() {
                self.select(Some(Target::Object(structure))).ok();
                self.active = Some(structure);
                changed |= self.duplicate_selected().is_ok();
            }
            if ui.button(locale.text("Delete")).clicked() {
                self.select(Some(Target::Object(structure))).ok();
                self.delete_selected();
                changed = true;
            }
        });
        changed
    }

    fn arrange_row(&mut self, ui: &mut egui::Ui) -> bool {
        let locale = self.locale.clone();
        let mut changed = false;
        ui.separator();
        ui.label(RichText::new(locale.text("Arrange")).strong());
        ui.horizontal(|ui| {
            if ui.button(locale.text("Align left")).clicked() {
                changed |= self
                    .align_selection(Axis::Horizontal, AlignEdge::Start)
                    .is_ok();
            }
            if ui.button(locale.text("Align center")).clicked() {
                changed |= self
                    .align_selection(Axis::Horizontal, AlignEdge::Center)
                    .is_ok();
            }
            if ui.button(locale.text("Align right")).clicked() {
                changed |= self
                    .align_selection(Axis::Horizontal, AlignEdge::End)
                    .is_ok();
            }
            if ui.button(locale.text("Distribute horizontally")).clicked() {
                changed |= self.distribute_selection(Axis::Horizontal).is_ok();
            }
        });
        ui.horizontal(|ui| {
            if ui.button(locale.text("Align top")).clicked() {
                changed |= self
                    .align_selection(Axis::Vertical, AlignEdge::Start)
                    .is_ok();
            }
            if ui.button(locale.text("Align middle")).clicked() {
                changed |= self
                    .align_selection(Axis::Vertical, AlignEdge::Center)
                    .is_ok();
            }
            if ui.button(locale.text("Align bottom")).clicked() {
                changed |= self.align_selection(Axis::Vertical, AlignEdge::End).is_ok();
            }
            if ui.button(locale.text("Distribute vertically")).clicked() {
                changed |= self.distribute_selection(Axis::Vertical).is_ok();
            }
        });
        changed
    }

    fn reaction_inspector(&mut self, ui: &mut egui::Ui, reaction: u64) -> bool {
        let locale = self.locale.clone();
        if self.source_id != Some(reaction) {
            self.source = self.reaction_source(reaction).unwrap_or_default();
            self.source_id = Some(reaction);
        }
        let Some(arrow) = self.reaction(reaction).map(|r| r.arrow.clone()) else {
            return false;
        };
        let mut changed = false;
        ui.label(RichText::new(locale.text("Reaction")).strong());
        ui.label(RichText::new(locale.text("mhchem")).strong());
        let edit = TextEdit::multiline(&mut self.source)
            .font(egui::TextStyle::Monospace)
            .desired_rows(3)
            .desired_width(f32::INFINITY);
        ui.add(edit);
        ui.horizontal(|ui| {
            let apply =
                egui::Button::new(RichText::new(locale.text("Apply")).color(Color32::WHITE))
                    .fill(ui.visuals().selection.bg_fill);
            if ui.add(apply).clicked() {
                match self.set_reaction_source(reaction, &self.source.clone()) {
                    Ok(()) => {
                        changed = true;
                        self.source = self.reaction_source(reaction).unwrap_or_default();
                    }
                    Err(error) => {
                        ui.colored_label(ui.visuals().error_fg_color, locale.diagnostic(&error));
                    }
                }
            }
            if ui.button(locale.text("Delete")).clicked() {
                self.select(Some(Target::Object(reaction))).ok();
                self.delete_selected();
                changed = true;
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(locale.text("Arrow"));
            egui::ComboBox::from_id_salt("chem-arrow-kind")
                .selected_text(locale.text(arrow_kind_key(arrow.kind)))
                .width(120.0)
                .show_ui(ui, |ui| {
                    for kind in [
                        ArrowKind::Right,
                        ArrowKind::Left,
                        ArrowKind::LeftRight,
                        ArrowKind::Equilibrium,
                        ArrowKind::Double,
                        ArrowKind::ReverseEquilibrium,
                    ] {
                        if ui
                            .selectable_label(arrow.kind == kind, locale.text(arrow_kind_key(kind)))
                            .clicked()
                        {
                            changed |= self
                                .set_reaction_arrow(
                                    reaction,
                                    kind,
                                    arrow.above.clone(),
                                    arrow.below.clone(),
                                )
                                .is_ok();
                        }
                    }
                });
        });
        let mut above = arrow.above.join(", ");
        let mut below = arrow.below.join(", ");
        ui.horizontal(|ui| {
            ui.label(locale.text("Above"));
            if ui.text_edit_singleline(&mut above).changed() {
                changed |= self
                    .set_reaction_arrow(
                        reaction,
                        arrow.kind,
                        split_conditions(&above),
                        arrow.below.clone(),
                    )
                    .is_ok();
            }
        });
        ui.horizontal(|ui| {
            ui.label(locale.text("Below"));
            if ui.text_edit_singleline(&mut below).changed() {
                changed |= self
                    .set_reaction_arrow(
                        reaction,
                        arrow.kind,
                        arrow.above.clone(),
                        split_conditions(&below),
                    )
                    .is_ok();
            }
        });
        ui.horizontal(|ui| {
            if ui.button(locale.text("Add species")).clicked() {
                self.add_species(reaction, true).ok();
                changed = true;
            }
        });
        if changed {
            self.source = self.reaction_source(reaction).unwrap_or_default();
        }
        changed
    }
}

fn object_bounds(object: &ChemObject) -> Option<(Point, Point)> {
    let theme = Theme::default();
    prims_bounds(&object_prims(object, &theme, false))
}

fn owning_object(target: Target) -> u64 {
    match target {
        Target::Object(id) => id,
        Target::Atom(structure, _) | Target::Bond(structure, _) => structure,
    }
}

fn highlight_target(
    painter: &egui::Painter,
    view: View,
    editor: &ChemEditor,
    target: Target,
    color: Color32,
    width: f32,
) {
    match target {
        Target::Atom(structure, atom) => {
            if let Some(point) = editor.atom_world(structure, atom) {
                painter.circle_stroke(
                    view.screen(point),
                    12.0 * view.zoom,
                    Stroke::new(width, color),
                );
            }
        }
        Target::Bond(structure, bond) => {
            if let Some(bond) = editor
                .structure(structure)
                .and_then(|s| s.bonds.iter().find(|b| b.id == bond))
            {
                if let (Some(a), Some(b)) = (
                    editor.atom_world(structure, bond.a),
                    editor.atom_world(structure, bond.b),
                ) {
                    painter.line_segment(
                        [view.screen(a), view.screen(b)],
                        Stroke::new((width + 3.0) * view.zoom, color.gamma_multiply(0.5)),
                    );
                }
            }
        }
        Target::Object(id) => {
            if let Some(object) = editor.document().object(id) {
                if let Some((min, max)) = object_bounds(object) {
                    let rect = Rect::from_min_max(
                        view.screen(Point::new(min.x - 10.0, min.y - 10.0)),
                        view.screen(Point::new(max.x + 10.0, max.y + 10.0)),
                    );
                    painter.rect_stroke(
                        rect,
                        CornerRadius::same(3),
                        Stroke::new(width, color),
                        StrokeKind::Outside,
                    );
                }
            }
        }
    }
}

fn distance_to_segment(p: Point, a: Point, b: Point) -> f32 {
    let ab = b.sub(a);
    let length_sq = ab.x * ab.x + ab.y * ab.y;
    if length_sq < 0.0001 {
        return p.distance(a);
    }
    let t = ((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / length_sq;
    let t = t.clamp(0.0, 1.0);
    p.distance(Point::new(a.x + ab.x * t, a.y + ab.y * t))
}

fn paint_prim(painter: &egui::Painter, view: View, prim: &Prim) {
    match prim {
        Prim::Line {
            from,
            to,
            width,
            color: stroke,
        } => {
            painter.line_segment(
                [view.screen(*from), view.screen(*to)],
                Stroke::new(width * view.zoom, color(*stroke)),
            );
        }
        Prim::Polygon { points, fill } => {
            let points: Vec<Pos2> = points.iter().map(|p| view.screen(*p)).collect();
            painter.add(Shape::convex_polygon(points, color(*fill), Stroke::NONE));
        }
        Prim::Text {
            pos,
            text,
            size,
            color: ink,
            align,
            ..
        } => {
            let anchor = match align {
                TextAlign::Left => Align2::LEFT_CENTER,
                TextAlign::Center => Align2::CENTER_CENTER,
                TextAlign::Right => Align2::RIGHT_CENTER,
            };
            painter.text(
                view.screen(*pos),
                anchor,
                text,
                FontId::proportional(size * view.zoom),
                color(*ink),
            );
        }
    }
}

fn split_conditions(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|piece| !piece.is_empty())
        .map(str::to_owned)
        .collect()
}

fn bond_order_key(order: BondOrder) -> &'static str {
    match order {
        BondOrder::Single => "Single",
        BondOrder::Double => "Double",
        BondOrder::Triple => "Triple",
        BondOrder::Aromatic => "Aromatic",
    }
}

fn stereo_key(stereo: BondStereo) -> &'static str {
    match stereo {
        BondStereo::Plain => "Plain",
        BondStereo::Wedge => "Wedge",
        BondStereo::Hash => "Hash",
    }
}

fn arrow_kind_key(kind: ArrowKind) -> &'static str {
    match kind {
        ArrowKind::Right => "Right",
        ArrowKind::Left => "Left",
        ArrowKind::LeftRight => "Left and right",
        ArrowKind::Equilibrium => "Equilibrium",
        ArrowKind::Double => "Double",
        ArrowKind::ReverseEquilibrium => "Reverse equilibrium",
    }
}

fn icon_button(ui: &mut egui::Ui, icon: Icon, tip: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(30.0, 26.0), Sense::click());
    paint_tool_frame(ui, rect, &response);
    let color = ui.style().interact(&response).fg_stroke.color;
    icons::paint(ui.painter(), rect.shrink(5.0), icon, color);
    response.on_hover_text(tip)
}

fn icon_toggle(ui: &mut egui::Ui, icon: Icon, tip: &str, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(30.0, 26.0), Sense::click());
    if selected {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(5),
            ui.visuals().selection.bg_fill.gamma_multiply(0.35),
        );
    }
    paint_tool_frame(ui, rect, &response);
    let color = ui.style().interact(&response).fg_stroke.color;
    icons::paint(ui.painter(), rect.shrink(5.0), icon, color);
    response.on_hover_text(tip)
}

fn paint_tool_frame(ui: &egui::Ui, rect: Rect, response: &egui::Response) {
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
        StrokeKind::Inside,
    );
}

fn element_button(ui: &mut egui::Ui, z: u8, locale: &crate::Locale) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(34.0, 26.0), Sense::click());
    let fill = element_fill(z);
    ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        element::symbol(z),
        FontId::proportional(14.0),
        Color32::from_rgb(20, 20, 20),
    );
    response.on_hover_text(locale.text(element::element(z).map_or("*", |info| info.name)))
}

fn element_popup(anchor: &egui::Response, title: &str, locale: &crate::Locale) -> Option<u8> {
    let mut picked = None;
    Popup::menu(anchor)
        .close_behavior(PopupCloseBehavior::CloseOnClick)
        .show(|ui| {
            ui.set_min_width(560.0);
            ui.label(RichText::new(title).strong());
            picked = periodic_table(ui, locale);
        });
    picked
}

/// The periodic table as an 18-column grid of clickable tiles.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn periodic_table(ui: &mut egui::Ui, locale: &crate::Locale) -> Option<u8> {
    const COLS: usize = 18;
    const ROWS: usize = 9;
    let cell = 27.0_f32;
    let gap = 2.0_f32;
    let size = Vec2::new(
        COLS as f32 * (cell + gap) - gap,
        ROWS as f32 * (cell + gap) - gap,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hover_pos().and_then(|pointer| {
        let column = ((pointer.x - rect.min.x) / (cell + gap)).floor();
        let row = ((pointer.y - rect.min.y) / (cell + gap)).floor();
        if column >= 0.0 && row >= 0.0 && (column as usize) < COLS && (row as usize) < ROWS {
            Some((column as usize, row as usize))
        } else {
            None
        }
    });
    let painter = ui.painter();
    for info in &ELEMENTS {
        let (column, row) = element::periodic_cell(info.z);
        let tile = Rect::from_min_size(
            rect.min
                + Vec2::new(
                    f32::from(column) * (cell + gap),
                    f32::from(row) * (cell + gap),
                ),
            Vec2::splat(cell),
        );
        let hot = hovered == Some((usize::from(column), usize::from(row)));
        let fill = if hot {
            ui.visuals().widgets.hovered.weak_bg_fill
        } else {
            element_fill(info.z)
        };
        painter.rect_filled(tile, 3.0, fill);
        painter.text(
            tile.center(),
            Align2::CENTER_CENTER,
            info.symbol,
            FontId::proportional(12.0),
            Color32::from_rgb(20, 20, 20),
        );
    }
    let hover_name = hovered
        .and_then(|(column, row)| element_at_cell(column, row))
        .map(|z| locale.text(element::element(z).map_or("*", |info| info.name)));
    let response = response.on_hover_text(hover_name.unwrap_or_default());
    if response.clicked() {
        if let Some((column, row)) = hovered {
            if let Some(z) = element_at_cell(column, row) {
                ui.close();
                return Some(z);
            }
        }
    }
    None
}

fn element_at_cell(column: usize, row: usize) -> Option<u8> {
    ELEMENTS.iter().find_map(|info| {
        let (c, r) = element::periodic_cell(info.z);
        (usize::from(c) == column && usize::from(r) == row).then_some(info.z)
    })
}

fn element_fill(z: u8) -> Color32 {
    let base = element::element(z).map_or([0xCC, 0xCC, 0xCC], element::ElementInfo::color);
    let light = [
        (u16::from(base[0]) + 255 * 2) / 3,
        (u16::from(base[1]) + 255 * 2) / 3,
        (u16::from(base[2]) + 255 * 2) / 3,
    ];
    Color32::from_rgb(light[0] as u8, light[1] as u8, light[2] as u8)
}
