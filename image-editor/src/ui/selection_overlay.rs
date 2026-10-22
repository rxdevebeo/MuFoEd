//! Bounded cached contour segments; checkerboard selections cannot grow UI memory without limit.
use super::ImageEditor;
use egui::{Color32, Rect, Stroke, Ui};
#[derive(Default)]
pub(super) struct SelectionOverlay {
    lines: Vec<[f32; 4]>,
    count: usize,
    coverage: Option<egui::TextureHandle>,
    bounds: Option<Rect>,
}
impl ImageEditor {
    #[allow(clippy::cast_precision_loss, clippy::many_single_char_names)]
    pub(super) fn rebuild_selection_outline(&mut self, ctx: &egui::Context) {
        let area = self.document.selection();
        self.selection_overlay.lines.clear();
        self.selection_overlay.count = 0;
        self.selection_overlay.coverage = None;
        self.selection_overlay.bounds = None;
        if area.is_empty() {
            return;
        }
        let w = self.document.width() as usize;
        let h = self.document.height() as usize;
        let inside = |x: usize, y: usize| area[y * w + x] >= 128;
        let mut bounds = [w, h, 0, 0];
        for (i, v) in area.iter().enumerate() {
            if *v > 0 {
                self.selection_overlay.count += 1;
                let x = i % w;
                let y = i / w;
                bounds[0] = bounds[0].min(x);
                bounds[1] = bounds[1].min(y);
                bounds[2] = bounds[2].max(x + 1);
                bounds[3] = bounds[3].max(y + 1);
            }
        }
        self.selection_overlay.bounds = Some(Rect::from_min_max(
            egui::pos2(bounds[0] as f32, bounds[1] as f32),
            egui::pos2(bounds[2] as f32, bounds[3] as f32),
        ));
        if matches!(
            self.manual_ui.tool,
            super::ManualTool::Wand | super::ManualTool::ColorSelect
        ) {
            let rgba: Vec<u8> = area.iter().flat_map(|&v| [50, 135, 255, v / 3]).collect();
            self.selection_overlay.coverage = Some(ctx.load_texture(
                "selection-coverage",
                egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba),
                egui::TextureOptions::NEAREST,
            ));
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
    pub(super) fn selection_pixel_count(&self) -> usize {
        self.selection_overlay.count
    }
    pub(super) fn paint_selection_outline(&self, ui: &Ui, rect: Rect) {
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        if self.manual_ui.show_selection_coverage
            && matches!(
                self.manual_ui.tool,
                super::ManualTool::Wand | super::ManualTool::ColorSelect
            )
        {
            if let Some(texture) = &self.selection_overlay.coverage {
                painter.image(
                    texture.id(),
                    rect,
                    Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            if let Some(bounds) = self.selection_overlay.bounds {
                let size = bounds.size() * self.zoom;
                if size.x < 4.0 || size.y < 4.0 {
                    let at = rect.min + bounds.center().to_vec2() * self.zoom;
                    painter.rect_stroke(
                        Rect::from_center_size(at, size.max(egui::Vec2::splat(5.0))),
                        0.0,
                        Stroke::new(2.0, Color32::WHITE),
                        egui::StrokeKind::Outside,
                    );
                }
            }
        }
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
