//! Small vector icons drawn with the egui painter, so the toolbar needs no
//! font glyphs or image assets.

use egui::{Color32, Pos2, Rect, Shape, Stroke, Vec2};

/// A toolbar icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Icon {
    Select,
    Move,
    Atom,
    Bond,
    Ring,
    Erase,
    Undo,
    Redo,
    ZoomIn,
    ZoomOut,
    Reset,
    Fit,
}

/// Draws `icon` inside `rect`.
#[allow(clippy::too_many_lines)]
pub(crate) fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    let center = rect.center();
    let r = rect.width().min(rect.height()) * 0.5;
    match icon {
        Icon::Select => {
            let points = vec![
                center + Vec2::new(-r * 0.55, -r * 0.7),
                center + Vec2::new(-r * 0.55, r * 0.7),
                center + Vec2::new(-r * 0.05, r * 0.25),
                center + Vec2::new(r * 0.4, r * 0.75),
                center + Vec2::new(r * 0.7, r * 0.5),
                center + Vec2::new(r * 0.2, r * 0.1),
                center + Vec2::new(r * 0.65, -r * 0.2),
            ];
            painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
        }
        Icon::Move => {
            let arm = r * 0.62;
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                let tip = center + Vec2::new(dx, dy) * arm;
                let back = center + Vec2::new(dx, dy) * (arm - r * 0.28);
                painter.line_segment([center, tip], stroke);
                let side = Vec2::new(dy, -dx) * r * 0.22;
                painter.line_segment([tip, back + side], stroke);
                painter.line_segment([tip, back - side], stroke);
            }
        }
        Icon::Atom => {
            painter.circle_stroke(center, r * 0.55, stroke);
            painter.circle_filled(center, r * 0.18, color);
        }
        Icon::Bond => {
            painter.line_segment(
                [
                    center + Vec2::new(-r * 0.7, 0.0),
                    center + Vec2::new(r * 0.7, 0.0),
                ],
                stroke,
            );
            painter.circle_filled(center + Vec2::new(-r * 0.7, 0.0), r * 0.18, color);
            painter.circle_filled(center + Vec2::new(r * 0.7, 0.0), r * 0.18, color);
        }
        Icon::Ring => {
            let points: Vec<Pos2> = (0..6)
                .map(|index| {
                    #[allow(clippy::cast_precision_loss)]
                    let angle = std::f32::consts::TAU * (index as f32) / 6.0;
                    center + Vec2::new(angle.cos(), angle.sin()) * r * 0.7
                })
                .collect();
            painter.add(Shape::closed_line(points, Stroke::new(1.4, color)));
        }
        Icon::Erase => {
            painter.rect_filled(
                Rect::from_center_size(center, Vec2::new(r * 1.4, r * 0.8)),
                2.0,
                color,
            );
            painter.line_segment(
                [
                    center + Vec2::new(-r * 0.7, r * 0.4),
                    center + Vec2::new(r * 0.7, -r * 0.4),
                ],
                Stroke::new(1.2, Color32::WHITE),
            );
        }
        Icon::Undo | Icon::Redo => {
            let flip = if icon == Icon::Redo { -1.0 } else { 1.0 };
            let a = center + Vec2::new(flip * r * 0.6, r * 0.3);
            let b = center + Vec2::new(-flip * r * 0.6, -r * 0.4);
            painter.line_segment([a, b], stroke);
            painter.line_segment([b, b + Vec2::new(-flip * r * 0.5, 0.0)], stroke);
            painter.line_segment([b, b + Vec2::new(0.0, r * 0.5)], stroke);
        }
        Icon::ZoomIn | Icon::ZoomOut => {
            painter.circle_stroke(center + Vec2::new(-r * 0.15, -r * 0.15), r * 0.5, stroke);
            painter.line_segment(
                [
                    center + Vec2::new(r * 0.25, r * 0.25),
                    center + Vec2::new(r * 0.7, r * 0.7),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    center + Vec2::new(-r * 0.5, -r * 0.15),
                    center + Vec2::new(r * 0.2, -r * 0.15),
                ],
                stroke,
            );
            if icon == Icon::ZoomIn {
                painter.line_segment(
                    [
                        center + Vec2::new(-r * 0.15, -r * 0.5),
                        center + Vec2::new(-r * 0.15, r * 0.2),
                    ],
                    stroke,
                );
            }
        }
        Icon::Reset => {
            painter.circle_stroke(center, r * 0.6, stroke);
            painter.circle_filled(center, r * 0.14, color);
        }
        Icon::Fit => {
            let frame = Rect::from_center_size(center, Vec2::splat(r * 1.3));
            painter.rect_stroke(frame, 1.5, stroke, egui::StrokeKind::Inside);
            let tick = r * 0.3;
            for (corner, sx, sy) in [
                (frame.left_top(), 1.0, 1.0),
                (frame.right_top(), -1.0, 1.0),
                (frame.left_bottom(), 1.0, -1.0),
                (frame.right_bottom(), -1.0, -1.0),
            ] {
                painter.line_segment([corner, corner + Vec2::new(sx * tick, 0.0)], stroke);
                painter.line_segment([corner, corner + Vec2::new(0.0, sy * tick)], stroke);
            }
        }
    }
}
