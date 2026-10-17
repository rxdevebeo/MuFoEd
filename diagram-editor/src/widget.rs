#![allow(clippy::too_many_lines, clippy::similar_names)]
use crate::{DiagramEditor, Node, Point, Port, ShapeKind, Target, Tool};
use egui::{
    Align2, Color32, FontId, Key, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2,
};

#[derive(Clone)]
pub(crate) struct Drag {
    node: Node,
    members: Vec<Node>,
    start: Point,
    resize: bool,
}
const BLUE: Color32 = Color32::from_rgb(43, 87, 154);
fn color(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}
fn painted_line(
    painter: &egui::Painter,
    points: &[Pos2],
    stroke: Stroke,
    dash: crate::Dash,
    zoom: f32,
) {
    if dash == crate::Dash::Solid {
        for pair in points.windows(2) {
            painter.line_segment([pair[0], pair[1]], stroke);
        }
        return;
    }
    let length = if dash == crate::Dash::Dotted {
        1.5
    } else {
        7.0
    } * zoom;
    let mut phase = 0.0;
    for pair in points.windows(2) {
        let vector = pair[1] - pair[0];
        let distance = vector.length();
        if distance < 0.001 {
            continue;
        }
        let clip = painter.clip_rect().expand(stroke.width);
        if !Rect::from_two_pos(pair[0], pair[1])
            .expand(stroke.width)
            .intersects(clip)
        {
            continue;
        }
        // Bound primitive generation even for imported scenes far outside the viewport.
        let step = (length + 4.0 * zoom).max(distance / 2048.0);
        let mut at = -phase;
        while at < distance {
            if at + length > 0.0 {
                painter.line_segment(
                    [
                        pair[0] + vector * (at.max(0.0) / distance),
                        pair[0] + vector * ((at + length).min(distance) / distance),
                    ],
                    stroke,
                );
            }
            at += step;
        }
        phase = (phase + distance).rem_euclid(step);
    }
}
fn cross(a: Pos2, b: Pos2, c: Pos2) -> f32 {
    let first = b - a;
    let second = c - a;
    first.x * second.y - first.y * second.x
}
fn triangles(points: &[Pos2]) -> Vec<[usize; 3]> {
    let mut indices: Vec<_> = (0..points.len()).collect();
    let mut result = vec![];
    let area: f32 = (0..points.len())
        .map(|i| {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            a.x * b.y - b.x * a.y
        })
        .sum();
    let sign = area.signum();
    while indices.len() > 3 {
        let mut ear = None;
        for i in 0..indices.len() {
            let a = indices[(i + indices.len() - 1) % indices.len()];
            let b = indices[i];
            let c = indices[(i + 1) % indices.len()];
            if cross(points[a], points[b], points[c]) * sign < 0.001 {
                continue;
            }
            let blocked = indices.iter().any(|index| {
                !([a, b, c].contains(index))
                    && [
                        cross(points[a], points[b], points[*index]),
                        cross(points[b], points[c], points[*index]),
                        cross(points[c], points[a], points[*index]),
                    ]
                    .iter()
                    .all(|v| v * sign >= -0.001)
            });
            if !blocked {
                ear = Some((i, [a, b, c]));
                break;
            }
        }
        let Some((index, triangle)) = ear else {
            break;
        };
        result.push(triangle);
        indices.remove(index);
    }
    if indices.len() == 3 {
        result.push([indices[0], indices[1], indices[2]]);
    }
    result
}
fn polygon(painter: &egui::Painter, points: Vec<Pos2>, fill: Color32, stroke: Stroke) {
    let mut mesh = egui::Mesh::default();
    for point in &points {
        mesh.colored_vertex(*point, fill);
    }
    for triangle in triangles(&points) {
        mesh.add_triangle(
            u32::try_from(triangle[0]).unwrap_or_default(),
            u32::try_from(triangle[1]).unwrap_or_default(),
            u32::try_from(triangle[2]).unwrap_or_default(),
        );
    }
    painter.add(egui::Shape::mesh(mesh));
    painter.add(egui::Shape::closed_line(points, stroke));
}
fn arrow(painter: &egui::Painter, a: Pos2, b: Pos2, color: Color32, zoom: f32) {
    let direction = (b - a).normalized();
    let normal = egui::vec2(-direction.y, direction.x);
    let back = b - direction * 10.0 * zoom;
    painter.add(egui::Shape::convex_polygon(
        vec![b, back + normal * 4.0 * zoom, back - normal * 4.0 * zoom],
        color,
        Stroke::NONE,
    ));
}
fn name(k: ShapeKind) -> &'static str {
    match k {
        ShapeKind::Rectangle => "Прямоугольник",
        ShapeKind::Rounded => "Скруглённый",
        ShapeKind::Ellipse => "Эллипс",
        ShapeKind::Diamond => "Ромб",
        ShapeKind::Text => "Текст",
        ShapeKind::Triangle => "Треугольник",
        ShapeKind::Trapezoid => "Трапеция",
        ShapeKind::Hexagon => "Шестиугольник",
        ShapeKind::Document => "Документ",
        ShapeKind::RightArrow => "Стрелка",
    }
}
fn port_name(p: Port) -> &'static str {
    match p {
        Port::Left => "Слева",
        Port::Right => "Справа",
        Port::Top => "Сверху",
        Port::Bottom => "Снизу",
    }
}
fn dash_combo(ui: &mut egui::Ui, id: &str, dash: &mut crate::Dash) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(match dash {
            crate::Dash::Solid => "Сплошная",
            crate::Dash::Dashed => "Штриховая",
            crate::Dash::Dotted => "Точечная",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(dash, crate::Dash::Solid, "Сплошная");
            ui.selectable_value(dash, crate::Dash::Dashed, "Штриховая");
            ui.selectable_value(dash, crate::Dash::Dotted, "Точечная");
        });
}
impl DiagramEditor {
    /// Draws the complete embeddable editor (tools, canvas and inspector).
    /// Returns whether an edit was committed during this frame.
    pub fn show(&mut self, ui: &mut egui::Ui) -> bool {
        let revision = self.revision;
        ui.horizontal_wrapped(|ui| {
            for (label, tool) in [
                ("Выбор", Tool::Select),
                ("Блок", Tool::Shape(ShapeKind::Rectangle)),
                ("Скруглённый", Tool::Shape(ShapeKind::Rounded)),
                ("Эллипс", Tool::Shape(ShapeKind::Ellipse)),
                ("Ромб", Tool::Shape(ShapeKind::Diamond)),
                ("Текст", Tool::Shape(ShapeKind::Text)),
                ("Треугольник", Tool::Shape(ShapeKind::Triangle)),
                ("Трапеция", Tool::Shape(ShapeKind::Trapezoid)),
                ("Шестиугольник", Tool::Shape(ShapeKind::Hexagon)),
                ("Документ", Tool::Shape(ShapeKind::Document)),
                ("Стрелка", Tool::Shape(ShapeKind::RightArrow)),
                ("Связь", Tool::Connect),
            ] {
                if ui.selectable_label(self.tool == tool, label).clicked() {
                    self.commit();
                    self.tool = tool;
                    self.pending_connection = None;
                }
            }
            ui.separator();
            if ui
                .add_enabled(
                    !self.undo.is_empty() || self.edit_base.is_some(),
                    egui::Button::new("Отменить"),
                )
                .on_hover_text("Отменить (Ctrl+Z)")
                .clicked()
            {
                self.undo();
            }
            if ui
                .add_enabled(!self.redo.is_empty(), egui::Button::new("Повторить"))
                .on_hover_text("Повторить (Ctrl+Y)")
                .clicked()
            {
                self.redo();
            }
            if ui
                .add_enabled(self.selection.is_some(), egui::Button::new("Удалить"))
                .clicked()
            {
                self.delete_selected();
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.selected_nodes().len() >= 2,
                    egui::Button::new("Группировать"),
                )
                .clicked()
            {
                let _ = self.group_selected();
            }
            if ui
                .add_enabled(
                    !self.selected_nodes().is_empty(),
                    egui::Button::new("Разгруппировать"),
                )
                .clicked()
            {
                self.ungroup_selected();
            }
            if ui.small_button("На передний план").clicked() {
                self.reorder_selected(true);
            }
            if ui.small_button("На задний план").clicked() {
                self.reorder_selected(false);
            }
            if ui
                .add_enabled(
                    !self.selected_nodes().is_empty(),
                    egui::Button::new("Дублировать"),
                )
                .clicked()
            {
                let _ = self.duplicate_selected();
            }
            ui.weak("Shift+щелчок: несколько фигур");
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.snap,"Привязка к сетке");
            if ui.small_button("−").clicked() { self.zoom = (self.zoom / 1.2).max(0.25); }
            ui.label(format!("{:.0}%",self.zoom*100.0));
            if ui.small_button("+").clicked() { self.zoom = (self.zoom * 1.2).min(3.0); }
            if ui.small_button("Сброс вида").clicked() { self.zoom=1.0; self.pan=egui::vec2(40.0,40.0); }
            ui.separator();
            ui.weak(match self.tool {
                Tool::Select => "Перенос мышью • маркер справа снизу • колесо: прокрутка • средняя кнопка: панорама",
                Tool::Connect => if self.pending_connection.is_some() { "Выберите конечную фигуру • Esc: отмена" } else { "Выберите начальную фигуру" },
                Tool::Shape(_) => "Щёлкните на холсте, чтобы добавить фигуру",
            });
        });
        ui.separator();
        let width = (ui.available_width() - 232.0).max(160.0);
        let height = ui.available_height().max(200.0);
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(width, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| self.canvas(ui),
            );
            ui.separator();
            ui.allocate_ui_with_layout(
                egui::vec2(212.0, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("diagram-properties")
                        .max_height(height)
                        .show(ui, |ui| self.inspector(ui));
                },
            );
        });
        self.revision != revision
    }
    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, Color32::WHITE);
        if response.hovered() {
            let (wheel, zoom, pointer) =
                ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
            if (zoom - 1.0).abs() > 0.001 {
                if let Some(pointer) = pointer {
                    let world = (pointer - rect.min - self.pan) / self.zoom;
                    self.zoom = (self.zoom * zoom).clamp(0.25, 3.0);
                    self.pan = pointer - rect.min - world * self.zoom;
                }
            } else {
                self.pan += wheel;
            }
        }
        if response.dragged_by(PointerButton::Middle) {
            self.pan += ui.input(|i| i.pointer.delta());
        }
        let origin = rect.min + self.pan;
        let zoom = self.zoom;
        let screen = |p: Point| origin + egui::vec2(p.x, p.y) * zoom;
        let world = |p: Pos2| {
            let p = (p - origin) / zoom;
            Point::new(p.x, p.y)
        };
        let spacing = 32.0 * self.zoom;
        let mut x = rect.left() + (origin.x - rect.left()).rem_euclid(spacing);
        while x < rect.right() {
            let mut y = rect.top() + (origin.y - rect.top()).rem_euclid(spacing);
            while y < rect.bottom() {
                painter.circle_filled(egui::pos2(x, y), 1.0, Color32::from_gray(223));
                y += spacing;
            }
            x += spacing;
        }
        let pointer = response
            .interact_pointer_pos()
            .or_else(|| ui.input(|i| i.pointer.hover_pos()));
        let p = pointer.map(world);
        // Update dragging from its original state, never from accumulated frame deltas.
        if response.drag_started_by(PointerButton::Primary) && self.tool == Tool::Select {
            self.commit();
            let start = ui.input(|i| i.pointer.press_origin()).map(world).or(p);
            if let Some(start) = start {
                let resize = self.selection_node().filter(|n| {
                    screen(Point::new(n.position.x + n.size.x, n.position.y + n.size.y))
                        .distance(screen(start))
                        < 12.0
                });
                let hit = self
                    .diagram
                    .nodes
                    .iter()
                    .rev()
                    .find(|n| n.contains(start))
                    .cloned();
                if let Some(hit) = resize.clone().or(hit) {
                    if !self.selected_nodes().contains(&hit.id) {
                        let _ = self.select(Some(Target::Node(hit.id)));
                    }
                    let node = self.selection_node().unwrap_or(hit);
                    let members = self
                        .diagram
                        .nodes
                        .iter()
                        .filter(|n| self.selected_nodes().contains(&n.id))
                        .cloned()
                        .collect();
                    let resize = resize.is_some();
                    self.begin();
                    self.drag = Some(Drag {
                        node,
                        members,
                        start,
                        resize,
                    });
                }
            }
        }
        if let (Some(drag), Some(p)) = (&self.drag, p) {
            let mut node = drag.node.clone();
            let snap = |v: f32| {
                if self.snap {
                    (v / 16.0).round() * 16.0
                } else {
                    v
                }
            };
            if drag.resize {
                node.size = Point::new(
                    snap(drag.node.size.x + p.x - drag.start.x).clamp(24.0, 10000.0),
                    snap(drag.node.size.y + p.y - drag.start.y).clamp(24.0, 10000.0),
                );
            } else {
                node.position = Point::new(
                    snap(drag.node.position.x + p.x - drag.start.x).clamp(-100_000.0, 100_000.0),
                    snap(drag.node.position.y + p.y - drag.start.y).clamp(-100_000.0, 100_000.0),
                );
            }
            let scale = Point::new(
                node.size.x / drag.node.size.x,
                node.size.y / drag.node.size.y,
            );
            let mut candidate = self.diagram.clone();
            for original in &drag.members {
                if let Some(n) = candidate.nodes.iter_mut().find(|n| n.id == original.id) {
                    n.position = Point::new(
                        node.position.x + (original.position.x - drag.node.position.x) * scale.x,
                        node.position.y + (original.position.y - drag.node.position.y) * scale.y,
                    );
                    n.size = Point::new(original.size.x * scale.x, original.size.y * scale.y);
                }
            }
            if candidate.validate().is_ok() {
                self.diagram = candidate;
            }
        }
        if self.drag.is_some() && !ui.input(|i| i.pointer.button_down(PointerButton::Primary)) {
            self.drag = None;
            self.commit();
        }
        if response.clicked() {
            self.commit();
            if let Some(p) = p {
                let hit = self
                    .diagram
                    .nodes
                    .iter()
                    .rev()
                    .find(|n| n.contains(p))
                    .map(|n| n.id);
                match self.tool {
                    Tool::Shape(kind) => {
                        let p = if self.snap {
                            Point::new((p.x / 16.0).round() * 16.0, (p.y / 16.0).round() * 16.0)
                        } else {
                            p
                        };
                        if self.add_node(kind, p).is_ok() {
                            self.tool = Tool::Select;
                            self.focus_text = true;
                        }
                    }
                    Tool::Connect => {
                        if let Some(id) = hit {
                            if let Some(from) = self.pending_connection.take() {
                                if from != id {
                                    let a = self.diagram.node(from).map(Node::center);
                                    let b = self.diagram.node(id).map(Node::center);
                                    if let (Some(a), Some(b)) = (a, b) {
                                        let (fp, tp) = if (b.x - a.x).abs() >= (b.y - a.y).abs() {
                                            if b.x >= a.x {
                                                (Port::Right, Port::Left)
                                            } else {
                                                (Port::Left, Port::Right)
                                            }
                                        } else if b.y >= a.y {
                                            (Port::Bottom, Port::Top)
                                        } else {
                                            (Port::Top, Port::Bottom)
                                        };
                                        let _ = self.connect(from, fp, id, tp);
                                        self.tool = Tool::Select;
                                    }
                                }
                            } else {
                                self.pending_connection = Some(id);
                            }
                        }
                    }
                    Tool::Select => {
                        let target = hit.map(Target::Node).or_else(|| {
                            self.diagram
                                .connections
                                .iter()
                                .rev()
                                .find(|c| {
                                    self.diagram.route(c).windows(2).any(|v| {
                                        segment_distance(screen(p), screen(v[0]), screen(v[1]))
                                            < 7.0
                                    })
                                })
                                .map(|c| Target::Connection(c.id))
                        });
                        if ui.input(|i| i.modifiers.shift) {
                            if let Some(id) = hit {
                                let mut ids = self.selected_nodes();
                                let clicked = self
                                    .diagram
                                    .groups
                                    .iter()
                                    .find(|g| g.members.contains(&id))
                                    .map_or_else(|| vec![id], |g| g.members.clone());
                                if clicked.iter().all(|id| ids.contains(id)) {
                                    for id in clicked {
                                        ids.remove(&id);
                                    }
                                } else {
                                    ids.extend(clicked);
                                }
                                let _ = self.select_nodes(&ids.into_iter().collect::<Vec<_>>());
                            }
                        } else {
                            let _ = self.select(target);
                        }
                        if response.double_clicked() && hit.is_some() {
                            self.focus_text = true;
                        }
                    }
                }
            }
        }
        if !ui.ctx().egui_wants_keyboard_input() {
            let (undo, redo, delete, escape) = ui.input(|i| {
                (
                    i.modifiers.command && !i.modifiers.shift && i.key_pressed(Key::Z),
                    i.modifiers.command
                        && (i.key_pressed(Key::Y) || (i.modifiers.shift && i.key_pressed(Key::Z))),
                    i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
                    i.key_pressed(Key::Escape),
                )
            });
            if undo {
                self.undo();
            } else if redo {
                self.redo();
            } else if delete {
                self.delete_selected();
            }
            if escape {
                if self.drag.take().is_some() {
                    if let Some(before) = self.edit_base.take() {
                        self.diagram = before;
                    }
                }
                self.pending_connection = None;
                self.tool = Tool::Select;
            }
        }
        let screen = |p: Point| origin + egui::vec2(p.x, p.y) * self.zoom;
        for c in &self.diagram.connections {
            let points: Vec<_> = self.diagram.route(c).into_iter().map(screen).collect();
            let selected = self.selection == Some(Target::Connection(c.id));
            let stroke = Stroke::new(
                c.width * self.zoom + if selected { 1.0 } else { 0.0 },
                color(c.color),
            );
            painted_line(&painter, &points, stroke, c.dash, self.zoom);
            if c.end_arrow {
                if let [.., a, b] = points.as_slice() {
                    arrow(&painter, *a, *b, color(c.color), self.zoom);
                }
            }
            if c.start_arrow {
                if let [a, b, ..] = points.as_slice() {
                    arrow(&painter, *b, *a, color(c.color), self.zoom);
                }
            }
        }
        if let Some((p, size)) = self.selection_frame() {
            if self.selected_nodes().len() > 1 {
                let group_frame =
                    Rect::from_min_size(screen(p), egui::vec2(size.x, size.y) * self.zoom);
                painter.rect_stroke(
                    group_frame.expand(4.0),
                    0.0,
                    Stroke::new(1.5, BLUE),
                    StrokeKind::Outside,
                );
                painter.rect_filled(
                    Rect::from_center_size(group_frame.right_bottom(), Vec2::splat(10.0)),
                    1.0,
                    BLUE,
                );
            }
        }
        for node in &self.diagram.nodes {
            let frame = Rect::from_min_size(
                screen(node.position),
                egui::vec2(node.size.x, node.size.y) * self.zoom,
            );
            let stroke = if node.style.no_stroke {
                Stroke::NONE
            } else {
                Stroke::new(node.style.width * self.zoom, color(node.stroke))
            };
            let actual_stroke = stroke;
            let stroke = if node.style.dash == crate::Dash::Solid {
                stroke
            } else {
                Stroke::NONE
            };
            let fill = if node.style.no_fill {
                Color32::TRANSPARENT
            } else {
                color(node.fill)
            };
            match node.kind {
                ShapeKind::Rectangle | ShapeKind::Rounded => {
                    painter.rect(
                        frame,
                        if node.kind == ShapeKind::Rounded {
                            node.size.x.min(node.size.y) / 6.0 * self.zoom
                        } else {
                            0.0
                        },
                        fill,
                        stroke,
                        StrokeKind::Inside,
                    );
                }
                ShapeKind::Ellipse => {
                    painter.add(egui::Shape::ellipse_filled(
                        frame.center(),
                        frame.size() / 2.0,
                        fill,
                    ));
                    painter.add(egui::Shape::ellipse_stroke(
                        frame.center(),
                        frame.size() / 2.0,
                        stroke,
                    ));
                }
                ShapeKind::Diamond
                | ShapeKind::Triangle
                | ShapeKind::Trapezoid
                | ShapeKind::Hexagon
                | ShapeKind::RightArrow
                | ShapeKind::Document => {
                    let points: Vec<_> = node.outline().into_iter().map(screen).collect();
                    polygon(&painter, points, fill, stroke);
                }
                ShapeKind::Text => {}
            }
            if node.style.dash != crate::Dash::Solid
                && !node.style.no_stroke
                && node.kind != ShapeKind::Text
            {
                let mut outline: Vec<_> = node.outline().into_iter().map(screen).collect();
                if let Some(first) = outline.first().copied() {
                    outline.push(first);
                }
                painted_line(
                    &painter,
                    &outline,
                    actual_stroke,
                    node.style.dash,
                    self.zoom,
                );
            }
            let mut job = egui::text::LayoutJob::simple(
                node.label_lines().join("\n"),
                FontId::proportional(node.font_size * self.zoom),
                Color32::from_gray(30),
                f32::INFINITY,
            );
            if let Some(section) = job.sections.first_mut() {
                section.format.italics = node.style.italic;
            }
            let galley = painter.layout_job(job);
            let text_x = match node.style.align {
                crate::TextAlign::Left => frame.left() + node.style.padding * self.zoom,
                crate::TextAlign::Center => frame.center().x - galley.size().x / 2.0,
                crate::TextAlign::Right => {
                    frame.right() - node.style.padding * self.zoom - galley.size().x
                }
            };
            let text_pos = egui::pos2(text_x, frame.center().y - galley.size().y / 2.0);
            let text_painter = painter.with_clip_rect(
                frame
                    .shrink(node.style.padding.min(frame.width() / 3.0) * self.zoom)
                    .intersect(rect),
            );
            if node.style.bold {
                text_painter.galley(
                    text_pos + egui::vec2(0.45 * self.zoom, 0.0),
                    galley.clone(),
                    Color32::from_gray(30),
                );
            }
            text_painter.galley(text_pos, galley, Color32::from_gray(30));
            if self.selected_nodes().contains(&node.id) || self.pending_connection == Some(node.id)
            {
                painter.rect_stroke(
                    frame.expand(3.0),
                    0.0,
                    Stroke::new(1.2, BLUE),
                    StrokeKind::Outside,
                );
                painter.rect_filled(
                    Rect::from_center_size(frame.right_bottom(), Vec2::splat(9.0)),
                    1.0,
                    BLUE,
                );
                if self.tool == Tool::Connect {
                    for port in [Port::Left, Port::Right, Port::Top, Port::Bottom] {
                        painter.circle_filled(screen(node.port(port)), 4.0, BLUE);
                    }
                }
            }
        }
        if self.diagram.nodes.is_empty() {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Выберите фигуру сверху и щёлкните на холсте",
                FontId::proportional(17.0),
                Color32::GRAY,
            );
        }
    }
    fn inspector(&mut self, ui: &mut egui::Ui) {
        ui.heading("Свойства");
        ui.add_space(8.0);
        match self.selection {
            Some(Target::Node(id)) => {
                let Some(mut node) = self.diagram.node(id).cloned() else {
                    return;
                };
                ui.label(name(node.kind));
                if self.selected_nodes().len() > 1 {
                    ui.label(format!("Выбрано фигур: {}", self.selected_nodes().len()));
                    if let Some((mut position, mut size)) = self.selection_frame() {
                        let old = (position, size);
                        for (label, value) in [
                            ("X", &mut position.x),
                            ("Y", &mut position.y),
                            ("Ширина", &mut size.x),
                            ("Высота", &mut size.y),
                        ] {
                            ui.horizontal(|ui| {
                                ui.label(label);
                                ui.add(egui::DragValue::new(value));
                            });
                        }
                        if old != (position, size) {
                            let _ = self.preview_transform(position, size);
                        }
                        if !ui.input(|i| i.pointer.any_down())
                            && !ui.ctx().egui_wants_keyboard_input()
                        {
                            self.commit();
                        }
                    }
                    ui.weak("Shift+щелчок добавляет фигуры. Группа переносится и меняет размер целиком.");
                    return;
                }
                ui.separator();
                ui.label("Текст");
                let text = ui.add(
                    egui::TextEdit::multiline(&mut node.text)
                        .id_salt(("diagram-text", id))
                        .desired_width(196.0)
                        .desired_rows(3)
                        .char_limit(2048),
                );
                if self.focus_text {
                    text.request_focus();
                    self.focus_text = false;
                }
                let mut changed = text.changed();
                let mut finished = text.lost_focus();
                for (label, value, range) in [
                    ("X", &mut node.position.x, -100_000.0..=100_000.0),
                    ("Y", &mut node.position.y, -100_000.0..=100_000.0),
                    ("Ширина", &mut node.size.x, 24.0..=10000.0),
                    ("Высота", &mut node.size.y, 24.0..=10000.0),
                    ("Кегль", &mut node.font_size, 8.0..=96.0),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(label);
                        let r = ui.add(egui::DragValue::new(value).range(range).speed(1.0));
                        changed |= r.changed();
                        finished |= r.drag_stopped() || r.lost_focus();
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Заливка");
                    changed |= ui.color_edit_button_srgb(&mut node.fill).changed();
                });
                ui.horizontal(|ui| {
                    ui.label("Контур");
                    changed |= ui.color_edit_button_srgb(&mut node.stroke).changed();
                });
                ui.checkbox(&mut node.style.no_fill, "Без заливки")
                    .changed()
                    .then(|| changed = true);
                changed |= ui
                    .checkbox(&mut node.style.no_stroke, "Без контура")
                    .changed();
                ui.horizontal(|ui| {
                    ui.label("Толщина");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut node.style.width)
                                .range(0.5..=20.0)
                                .speed(0.1),
                        )
                        .changed();
                });
                let old_style = node.style.clone();
                dash_combo(ui, "Контур", &mut node.style.dash);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut node.style.bold, "Жирный");
                    ui.checkbox(&mut node.style.italic, "Курсив");
                });
                egui::ComboBox::from_id_salt("text-align")
                    .selected_text(match node.style.align {
                        crate::TextAlign::Left => "Текст слева",
                        crate::TextAlign::Center => "Текст по центру",
                        crate::TextAlign::Right => "Текст справа",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut node.style.align, crate::TextAlign::Left, "Слева");
                        ui.selectable_value(
                            &mut node.style.align,
                            crate::TextAlign::Center,
                            "По центру",
                        );
                        ui.selectable_value(
                            &mut node.style.align,
                            crate::TextAlign::Right,
                            "Справа",
                        );
                    });
                ui.checkbox(&mut node.style.wrap, "Перенос строк");
                ui.horizontal(|ui| {
                    ui.label("Поля");
                    ui.add(egui::DragValue::new(&mut node.style.padding).range(0.0..=100.0));
                });
                changed |= node.style != old_style;
                if changed {
                    self.begin();
                    if let Some(n) = self.diagram.nodes.iter_mut().find(|n| n.id == id) {
                        *n = node;
                    }
                }
                if finished
                    || (self.edit_base.is_some()
                        && !ui.ctx().egui_wants_keyboard_input()
                        && !ui.input(|i| i.pointer.any_down()))
                {
                    self.commit();
                }
                ui.add_space(12.0);
                ui.weak("Для размера тяните синий маркер справа снизу. Двойной щелчок переводит фокус в текст.");
            }
            Some(Target::Connection(id)) => {
                let Some(mut c) = self
                    .diagram
                    .connections
                    .iter()
                    .find(|c| c.id == id)
                    .cloned()
                else {
                    return;
                };
                ui.label("Связанная стрелка");
                ui.label(format!("Из {} в {}", c.from, c.to));
                let before = c.clone();
                for (label, port) in [("Начало", &mut c.from_port), ("Конец", &mut c.to_port)]
                {
                    egui::ComboBox::from_id_salt(("port", id, label))
                        .selected_text(format!("{label}: {}", port_name(*port)))
                        .show_ui(ui, |ui| {
                            for p in [Port::Left, Port::Right, Port::Top, Port::Bottom] {
                                ui.selectable_value(port, p, port_name(p));
                            }
                        });
                }
                ui.color_edit_button_srgb(&mut c.color);
                ui.horizontal(|ui| {
                    ui.label("Толщина");
                    ui.add(
                        egui::DragValue::new(&mut c.width)
                            .range(0.5..=20.0)
                            .speed(0.1),
                    );
                });
                dash_combo(ui, "Линия", &mut c.dash);
                ui.checkbox(&mut c.start_arrow, "Стрелка в начале");
                ui.checkbox(&mut c.end_arrow, "Стрелка в конце");
                if c != before {
                    self.begin();
                    if let Some(link) = self.diagram.connections.iter_mut().find(|c| c.id == id) {
                        *link = c;
                    }
                }
                if !ui.input(|i| i.pointer.any_down()) && !ui.ctx().egui_wants_keyboard_input() {
                    self.commit();
                }
                ui.weak("Связь сохраняется при переносе и изменении размера фигур.");
            }
            None => {
                ui.weak("Выберите фигуру или стрелку.");
                ui.add_space(12.0);
                ui.label(format!(
                    "Фигур: {}\nСвязей: {}",
                    self.diagram.nodes.len(),
                    self.diagram.connections.len()
                ));
            }
        }
    }
}
fn segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let v = b - a;
    let len = v.length_sq();
    if len < 0.001 {
        return p.distance(a);
    }
    p.distance(a + v * ((p - a).dot(v) / len).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concave_presets_are_filled_by_triangles_with_the_same_total_area() {
        let mut editor = DiagramEditor::default();
        for kind in [
            ShapeKind::Document,
            ShapeKind::RightArrow,
            ShapeKind::Triangle,
            ShapeKind::Rounded,
        ] {
            let id = editor.add_node(kind, Point::new(0.0, 0.0)).unwrap();
            let points: Vec<_> = editor
                .diagram
                .node(id)
                .unwrap()
                .outline()
                .into_iter()
                .map(|p| egui::pos2(p.x, p.y))
                .collect();
            let mesh = triangles(&points);
            assert_eq!(mesh.len(), points.len() - 2);
            let expected = (0..points.len())
                .map(|i| {
                    let a = points[i];
                    let b = points[(i + 1) % points.len()];
                    a.x * b.y - b.x * a.y
                })
                .sum::<f32>()
                .abs()
                / 2.0;
            let actual = mesh
                .iter()
                .map(|[a, b, c]| cross(points[*a], points[*b], points[*c]).abs() / 2.0)
                .sum::<f32>();
            assert!((actual - expected).abs() < 0.05);
        }
    }
    fn frame(ctx: &egui::Context, editor: &mut DiagramEditor, events: Vec<egui::Event>) -> Pos2 {
        let mut origin = Pos2::ZERO;
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 600.0))),
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            origin = ui.available_rect_before_wrap().min + editor.pan;
            editor.canvas(ui);
        });
        origin
    }
    fn button(p: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        }
    }
    #[test]
    fn dragging_and_resizing_are_single_undo_actions_and_delete_restores_links() {
        let ctx = egui::Context::default();
        let mut editor = DiagramEditor::sample();
        let origin = frame(&ctx, &mut editor, vec![]);
        let id = editor.diagram.nodes[0].id;
        let center = origin + egui::vec2(160.0, 120.0);
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerMoved(center), button(center, true)],
        );
        frame(&ctx, &mut editor, vec![button(center, false)]);
        assert_eq!(editor.selection, Some(Target::Node(id)));
        let before = editor.diagram.clone();
        let count = editor.undo.len();
        frame(&ctx, &mut editor, vec![button(center, true)]);
        let moved = center + egui::vec2(64.0, 48.0);
        frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(moved)]);
        frame(&ctx, &mut editor, vec![button(moved, false)]);
        assert_eq!(
            editor.diagram.node(id).unwrap().position,
            Point::new(144.0, 128.0)
        );
        assert_eq!(editor.undo.len(), count + 1);
        editor.undo();
        assert_eq!(editor.diagram, before);
        editor.redo();
        editor.selection = Some(Target::Node(id));
        let before_resize = editor.diagram.clone();
        let node = editor.diagram.node(id).unwrap();
        let corner =
            origin + egui::vec2(node.position.x + node.size.x, node.position.y + node.size.y);
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerMoved(corner), button(corner, true)],
        );
        let end = corner + egui::vec2(64.0, 32.0);
        frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(end)]);
        frame(&ctx, &mut editor, vec![button(end, false)]);
        assert_eq!(
            editor.diagram.node(id).unwrap().size,
            Point::new(224.0, 112.0)
        );
        editor.undo();
        assert_eq!(editor.diagram, before_resize);
        editor.selection = Some(Target::Node(id));
        editor.delete_selected();
        assert!(editor.diagram.node(id).is_none());
        assert!(editor
            .diagram
            .connections
            .iter()
            .all(|c| c.from != id && c.to != id));
        editor.undo();
        assert_eq!(editor.diagram, before_resize);
    }
    #[test]
    fn cancelling_a_drag_restores_geometry_without_adding_history() {
        let ctx = egui::Context::default();
        let mut editor = DiagramEditor::sample();
        let origin = frame(&ctx, &mut editor, vec![]);
        let before = editor.diagram.clone();
        let p = origin + egui::vec2(160.0, 120.0);
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerMoved(p), button(p, true)],
        );
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerMoved(p + egui::vec2(64.0, 48.0))],
        );
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::Key {
                key: Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            }],
        );
        assert_eq!(editor.diagram, before);
        assert!(editor.undo.is_empty());
    }

    #[test]
    fn placing_shapes_and_connecting_by_click_works_after_zoom_and_pan() {
        let ctx = egui::Context::default();
        let mut editor = DiagramEditor {
            zoom: 1.5,
            pan: egui::vec2(32.0, 24.0),
            ..DiagramEditor::default()
        };
        let origin = frame(&ctx, &mut editor, vec![]);
        for (kind, world) in [
            (ShapeKind::Ellipse, Point::new(32.0, 32.0)),
            (ShapeKind::Rectangle, Point::new(272.0, 32.0)),
        ] {
            editor.tool = Tool::Shape(kind);
            let p = origin + egui::vec2(world.x, world.y) * editor.zoom;
            frame(
                &ctx,
                &mut editor,
                vec![egui::Event::PointerMoved(p), button(p, true)],
            );
            frame(&ctx, &mut editor, vec![button(p, false)]);
            assert_eq!(editor.diagram.nodes.last().unwrap().position, world);
        }
        editor.tool = Tool::Connect;
        let centers: Vec<_> = editor
            .diagram
            .nodes
            .iter()
            .map(|n| origin + egui::vec2(n.center().x, n.center().y) * editor.zoom)
            .collect();
        for p in centers {
            frame(
                &ctx,
                &mut editor,
                vec![egui::Event::PointerMoved(p), button(p, true)],
            );
            frame(&ctx, &mut editor, vec![button(p, false)]);
        }
        assert_eq!(editor.diagram.connections.len(), 1);
        let before = editor.diagram.clone();
        editor.undo();
        assert!(editor.diagram.connections.is_empty());
        assert_eq!(editor.diagram.nodes.len(), 2);
        editor.redo();
        assert_eq!(editor.diagram, before);
    }
    #[test]
    fn grouped_drag_and_resize_preserve_relative_geometry_and_undo_once() {
        let ctx = egui::Context::default();
        let mut editor = DiagramEditor::sample();
        let ids: Vec<_> = editor.diagram.nodes[..2].iter().map(|n| n.id).collect();
        editor.select_nodes(&ids).unwrap();
        editor.group_selected().unwrap();
        let origin = frame(&ctx, &mut editor, vec![]);
        let before = editor.diagram.clone();
        let depth = editor.undo.len();
        let start = origin + egui::vec2(160.0, 120.0);
        let moved = start + egui::vec2(64.0, 48.0);
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerMoved(start), button(start, true)],
        );
        frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(moved)]);
        frame(&ctx, &mut editor, vec![button(moved, false)]);
        for id in &ids {
            let expected = before.node(*id).unwrap();
            let actual = editor.diagram.node(*id).unwrap();
            assert_eq!(
                actual.position,
                Point::new(expected.position.x + 64.0, expected.position.y + 48.0)
            );
        }
        assert_eq!(editor.undo.len(), depth + 1);
        editor.undo();
        assert_eq!(editor.diagram, before);
        editor.select_nodes(&ids).unwrap();
        let (position, size) = editor.selection_frame().unwrap();
        let corner = origin + egui::vec2(position.x + size.x, position.y + size.y);
        let end = corner + egui::vec2(size.x, size.y);
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerMoved(corner), button(corner, true)],
        );
        frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(end)]);
        frame(&ctx, &mut editor, vec![button(end, false)]);
        for id in &ids {
            assert_eq!(
                editor.diagram.node(*id).unwrap().size,
                Point::new(320.0, 160.0)
            );
        }
        editor.undo();
        assert_eq!(editor.diagram, before);
    }
}
