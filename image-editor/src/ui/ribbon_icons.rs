//! Small monochrome ribbon glyphs drawn as vectors; no font/emoji dependency.
use egui::{Color32, Pos2, Rect, Response, Stroke, Ui, Vec2};
#[derive(Clone, Copy)]
pub(super) enum Icon {
    Copy,
    Cut,
    Paste,
    Delete,
    Undo,
    Redo,
    Import,
    New,
    Duplicate,
    Canvas,
    Rotate,
    Crop,
    Transform,
    FlipH,
    FlipV,
    Levels,
    Curves,
    Adjust,
    Clear,
    Keep,
    Settings,
    Memory,
    Fit,
    More,
}
pub(super) fn button(ui: &mut Ui, icon: Icon, tip: &str) -> Response {
    sized_button(ui, icon, tip, 32.0)
}
pub(super) fn compact_button(ui: &mut Ui, icon: Icon, tip: &str) -> Response {
    sized_button(ui, icon, tip, 24.0)
}
fn sized_button(ui: &mut Ui, icon: Icon, tip: &str, size: f32) -> Response {
    let response = ui
        .add_sized([size, size], egui::Button::new(""))
        .on_hover_text(tip);
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tip));
    paint(
        ui.painter(),
        response.rect.shrink(size * 7.0 / 32.0),
        icon,
        ui.visuals().text_color(),
    );
    response
}
pub(super) fn row(ui: &mut Ui, icon: Icon, label: &str, tip: &str) -> bool {
    let response = ui
        .add_sized(
            [ui.available_width().min(150.0), 24.0],
            egui::Button::new(""),
        )
        .on_hover_text(tip);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    paint(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(response.rect.left() + 13.0, response.rect.center().y),
            Vec2::splat(16.0),
        ),
        icon,
        ui.visuals().text_color(),
    );
    ui.painter().text(
        Pos2::new(response.rect.left() + 26.0, response.rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        ui.visuals().text_color(),
    );
    response.clicked()
}
#[allow(clippy::too_many_lines, clippy::many_single_char_names)] // One normalized vector path per recognizable glyph.
fn paint(p: &egui::Painter, r: Rect, icon: Icon, c: Color32) {
    let point = |x: f32, y: f32| r.min + Vec2::new(x * r.width(), y * r.height());
    let s = Stroke::new(1.5, c);
    let line = |a: [f32; 2], b: [f32; 2]| {
        p.line_segment([point(a[0], a[1]), point(b[0], b[1])], s);
    };
    let rect = |a: [f32; 2], b: [f32; 2]| {
        p.rect_stroke(
            Rect::from_min_max(point(a[0], a[1]), point(b[0], b[1])),
            1.0,
            s,
            egui::StrokeKind::Inside,
        );
    };
    match icon {
        Icon::Copy | Icon::Duplicate => {
            rect([0.05, 0.05], [0.68, 0.72]);
            rect([0.3, 0.28], [0.95, 0.95]);
        }
        Icon::Cut => {
            p.circle_stroke(point(0.2, 0.8), r.width() * 0.15, s);
            p.circle_stroke(point(0.8, 0.8), r.width() * 0.15, s);
            line([0.3, 0.68], [0.9, 0.05]);
            line([0.7, 0.68], [0.1, 0.05]);
        }
        Icon::Paste => {
            rect([0.13, 0.15], [0.87, 0.95]);
            rect([0.32, 0.02], [0.68, 0.25]);
            line([0.3, 0.48], [0.7, 0.48]);
            line([0.3, 0.7], [0.7, 0.7]);
        }
        Icon::Delete => {
            line([0.05, 0.22], [0.95, 0.22]);
            rect([0.35, 0.02], [0.65, 0.22]);
            line([0.2, 0.3], [0.28, 0.95]);
            line([0.28, 0.95], [0.72, 0.95]);
            line([0.72, 0.95], [0.8, 0.3]);
            line([0.4, 0.4], [0.4, 0.78]);
            line([0.6, 0.4], [0.6, 0.78]);
        }
        Icon::Undo | Icon::Redo => {
            let flip = matches!(icon, Icon::Redo);
            let q = |x: f32, y: f32| point(if flip { 1.0 - x } else { x }, y);
            p.add(egui::Shape::line(
                vec![q(0.1, 0.4), q(0.55, 0.4), q(0.85, 0.55), q(0.85, 0.85)],
                s,
            ));
            p.add(egui::Shape::line(
                vec![q(0.36, 0.12), q(0.08, 0.4), q(0.36, 0.68)],
                s,
            ));
        }
        Icon::New | Icon::Import => {
            rect([0.12, 0.05], [0.88, 0.95]);
            line([0.5, 0.3], [0.5, 0.75]);
            line([0.3, 0.52], [0.7, 0.52]);
            if matches!(icon, Icon::Import) {
                line([0.32, 0.65], [0.5, 0.82]);
                line([0.5, 0.82], [0.68, 0.65]);
            }
        }
        Icon::Canvas | Icon::Transform => {
            rect([0.08, 0.08], [0.92, 0.92]);
            for [x, y] in [[0.08, 0.08], [0.92, 0.08], [0.08, 0.92], [0.92, 0.92]] {
                p.rect_filled(
                    Rect::from_center_size(point(x, y), Vec2::splat(4.0)),
                    0.0,
                    c,
                );
            }
        }
        Icon::Rotate => {
            p.circle_stroke(point(0.5, 0.55), r.width() * 0.35, s);
            line([0.65, 0.02], [0.9, 0.2]);
            line([0.9, 0.2], [0.65, 0.35]);
        }
        Icon::Crop => {
            line([0.25, 0.0], [0.25, 0.75]);
            line([0.25, 0.75], [1.0, 0.75]);
            line([0.0, 0.25], [0.75, 0.25]);
            line([0.75, 0.25], [0.75, 1.0]);
        }
        Icon::FlipH | Icon::FlipV => {
            let v = matches!(icon, Icon::FlipV);
            let q = |x: f32, y: f32| if v { point(y, x) } else { point(x, y) };
            p.line_segment([q(0.5, 0.0), q(0.5, 1.0)], s);
            for points in [
                [[0.05, 0.18], [0.35, 0.5], [0.05, 0.82]],
                [[0.95, 0.18], [0.65, 0.5], [0.95, 0.82]],
            ] {
                p.add(egui::Shape::closed_line(
                    points.map(|a| q(a[0], a[1])).to_vec(),
                    s,
                ));
            }
        }
        Icon::Levels => {
            line([0.02, 0.95], [0.98, 0.95]);
            for (x, h) in [
                (0.12, 0.4),
                (0.32, 0.7),
                (0.52, 0.95),
                (0.72, 0.6),
                (0.92, 0.3),
            ] {
                line([x, 0.9], [x, 1.0 - h]);
            }
        }
        Icon::Curves => {
            rect([0.05, 0.05], [0.95, 0.95]);
            p.add(egui::Shape::line(
                vec![
                    point(0.12, 0.87),
                    point(0.35, 0.8),
                    point(0.55, 0.35),
                    point(0.87, 0.12),
                ],
                s,
            ));
        }
        Icon::Adjust | Icon::Settings => {
            for (y, x) in [(0.2, 0.7), (0.5, 0.3), (0.8, 0.6)] {
                line([0.0, y], [1.0, y]);
                p.circle_filled(point(x, y), 3.0, c);
            }
        }
        Icon::Clear => {
            rect([0.05, 0.05], [0.95, 0.95]);
            line([0.23, 0.23], [0.77, 0.77]);
            line([0.77, 0.23], [0.23, 0.77]);
        }
        Icon::Keep => {
            rect([0.05, 0.05], [0.72, 0.72]);
            line([0.3, 0.95], [0.95, 0.95]);
            line([0.95, 0.95], [0.95, 0.3]);
        }
        Icon::Memory => {
            rect([0.2, 0.2], [0.8, 0.8]);
            for f in [0.3, 0.5, 0.7] {
                line([f, 0.0], [f, 0.2]);
                line([f, 0.8], [f, 1.0]);
                line([0.0, f], [0.2, f]);
                line([0.8, f], [1.0, f]);
            }
        }
        Icon::More => {
            line([0.15, 0.35], [0.5, 0.7]);
            line([0.5, 0.7], [0.85, 0.35]);
        }
        Icon::Fit => {
            for (x, y) in [(0.05, 0.05), (0.95, 0.05), (0.05, 0.95), (0.95, 0.95)] {
                let dx = if x < 0.5 { 0.25 } else { -0.25 };
                let dy = if y < 0.5 { 0.25 } else { -0.25 };
                line([x, y], [x + dx, y]);
                line([x, y], [x, y + dy]);
            }
        }
    }
}
