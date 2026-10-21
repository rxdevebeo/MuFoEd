//! Bounded cached contour segments; checkerboard selections cannot grow UI memory without limit.
use super::ImageEditor;
use egui::{Color32, Rect, Stroke, Ui};
#[derive(Default)]
pub(super) struct SelectionOverlay {
    lines: Vec<[f32; 4]>,
}
impl ImageEditor {
    #[allow(clippy::cast_precision_loss, clippy::many_single_char_names)]
    pub(super) fn rebuild_selection_outline(&mut self) {
        let area = self.document.selection();
        self.selection_overlay.lines.clear();
        if area.is_empty() {
            return;
        }
        let w = self.document.width() as usize;
        let h = self.document.height() as usize;
        let inside = |x: usize, y: usize| area[y * w + x] >= 128;
        let mut bounds = [w, h, 0, 0];
        for (i, v) in area.iter().enumerate() {
            if *v > 0 {
                let x = i % w;
                let y = i / w;
                bounds[0] = bounds[0].min(x);
                bounds[1] = bounds[1].min(y);
                bounds[2] = bounds[2].max(x + 1);
                bounds[3] = bounds[3].max(y + 1);
            }
        }
        for y in 0..=h {
            let mut start = None;
            for x in 0..=w {
                let edge = x < w && ((y > 0 && inside(x, y - 1)) != (y < h && inside(x, y)));
                if edge {
                    start.get_or_insert(x);
                } else if let Some(a) = start.take() {
                    self.selection_overlay
                        .lines
                        .push([a as f32, y as f32, x as f32, y as f32]);
                }
            }
            if self.selection_overlay.lines.len() > 8192 {
                break;
            }
        }
        if self.selection_overlay.lines.len() <= 8192 {
            for x in 0..=w {
                let mut start = None;
                for y in 0..=h {
                    let edge = y < h && ((x > 0 && inside(x - 1, y)) != (x < w && inside(x, y)));
                    if edge {
                        start.get_or_insert(y);
                    } else if let Some(a) = start.take() {
                        self.selection_overlay
                            .lines
                            .push([x as f32, a as f32, x as f32, y as f32]);
                    }
                }
                if self.selection_overlay.lines.len() > 8192 {
                    break;
                }
            }
        }
        if self.selection_overlay.lines.len() > 8192
            || (self.selection_overlay.lines.is_empty() && bounds[2] > 0)
        {
            let [l, t, r, b] = bounds.map(|v| v as f32);
            self.selection_overlay.lines =
                vec![[l, t, r, t], [r, t, r, b], [r, b, l, b], [l, b, l, t]];
        }
    }
    pub(super) fn paint_selection_outline(&self, ui: &Ui, rect: Rect) {
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        for [x0, y0, x1, y1] in &self.selection_overlay.lines {
            let line = [
                rect.min + egui::vec2(*x0, *y0) * self.zoom,
                rect.min + egui::vec2(*x1, *y1) * self.zoom,
            ];
            painter.line_segment(line, Stroke::new(2.0, Color32::BLACK));
            painter.extend(egui::Shape::dashed_line(
                &line,
                Stroke::new(1.0, Color32::WHITE),
                4.0,
                4.0,
            ));
        }
    }
}
