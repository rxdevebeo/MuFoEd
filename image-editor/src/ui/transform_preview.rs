//! GPU mesh previews use original textures; only Apply resamples document pixels.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
use super::ImageEditor;
use egui::{Color32, Rect, Vec2};
const N: u32 = 128;
#[derive(Default)]
pub(super) struct TransformPreview {
    pub layers: Vec<egui::TextureHandle>,
}
impl ImageEditor {
    pub(super) fn transform_preview_active(&self) -> bool {
        self.layer_preview().is_some() || self.rotation_preview().is_some()
    }
    pub(super) fn preview_canvas_size(&self) -> Vec2 {
        let w = f64::from(self.document.width());
        let h = f64::from(self.document.height());
        if let Some((angle, true)) = self.rotation_preview() {
            let (sin, cos) = angle.to_radians().sin_cos();
            return egui::vec2(
                (w * cos.abs() + h * sin.abs() - 1e-10).ceil() as f32,
                (w * sin.abs() + h * cos.abs() - 1e-10).ceil() as f32,
            );
        }
        egui::vec2(w as f32, h as f32)
    }
    #[allow(clippy::too_many_lines)] // Shared GPU mesh path for raster layers and mask overlay.
    pub(super) fn paint_transform_preview(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        rect: Rect,
    ) -> bool {
        if let Some((angle, _)) = self.rotation_preview() {
            let size =
                egui::vec2(self.document.width() as f32, self.document.height() as f32) * self.zoom;
            let (sin, cos) = (angle as f32).to_radians().sin_cos();
            let mut mesh = egui::Mesh::with_texture(self.texture.as_ref().expect("canvas").id());
            for uv in [
                egui::pos2(0.0, 0.0),
                egui::pos2(1.0, 0.0),
                egui::pos2(0.0, 1.0),
                egui::pos2(1.0, 1.0),
            ] {
                let d = (uv - egui::pos2(0.5, 0.5)) * size;
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: rect.center() + egui::vec2(cos * d.x - sin * d.y, sin * d.x + cos * d.y),
                    uv,
                    color: Color32::WHITE,
                });
            }
            mesh.indices.extend([0, 1, 2, 2, 1, 3]);
            painter.add(mesh);
        } else if let Some((angle, scale, photo, center)) = self.layer_preview() {
            if self.transform_preview.layers.is_empty() {
                self.load_preview_layers(ui);
            }
            let dimensions = [self.document.width(), self.document.height()];
            let mask = self.mask_transform_preview();
            let active = if mask { 0 } else { self.document.active() };
            let layers: Vec<_> = if mask {
                painter.image(
                    self.texture.as_ref().expect("canvas").id(),
                    rect,
                    Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                vec![(0, 255)]
            } else {
                self.document
                    .layers()
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| l.visible())
                    .map(|(i, l)| (i, l.opacity()))
                    .collect()
            };
            for (i, opacity) in layers {
                let texture = &self.transform_preview.layers[i];
                let color = Color32::from_white_alpha(opacity);
                if i == active {
                    let mut mesh = egui::Mesh::with_texture(texture.id());
                    for y in 0..=N {
                        for x in 0..=N {
                            let f = egui::vec2(x as f32 / N as f32, y as f32 / N as f32);
                            let point = [
                                f64::from(f.x) * f64::from(dimensions[0]),
                                f64::from(f.y) * f64::from(dimensions[1]),
                            ];
                            let source =
                                photo.source_point(point, dimensions, angle, scale, center);
                            let uv = egui::pos2(
                                ((source[0] + 1.0) / f64::from(dimensions[0] + 2)) as f32,
                                ((source[1] + 1.0) / f64::from(dimensions[1] + 2)) as f32,
                            );
                            mesh.vertices.push(egui::epaint::Vertex {
                                pos: rect.min + f * rect.size(),
                                uv,
                                color,
                            });
                        }
                    }
                    for y in 0..N {
                        for x in 0..N {
                            let a = y * (N + 1) + x;
                            let b = a + N + 1;
                            mesh.indices.extend([a, a + 1, b, b, a + 1, b + 1]);
                        }
                    }
                    painter.add(mesh);
                } else {
                    let size = texture.size_vec2();
                    painter.image(
                        texture.id(),
                        rect,
                        Rect::from_min_max(
                            egui::pos2(1.0 / size.x, 1.0 / size.y),
                            egui::pos2(1.0 - 1.0 / size.x, 1.0 - 1.0 / size.y),
                        ),
                        color,
                    );
                }
            }
        } else {
            self.transform_preview.layers.clear();
            return false;
        }
        // Straight reference lines make sub-degree rotations and perspective visible.
        for i in 1..10 {
            let f = i as f32 / 10.0;
            let x = rect.left() + rect.width() * f;
            let y = rect.top() + rect.height() * f;
            let stroke = egui::Stroke::new(1.0, Color32::from_white_alpha(75));
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                stroke,
            );
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                stroke,
            );
        }
        true
    }
    fn load_preview_layers(&mut self, ui: &egui::Ui) {
        let rasters: Vec<_> = if self.mask_transform_preview() {
            let pixels = self
                .document
                .mask()
                .iter()
                .flat_map(|v| [255, 0, 0, *v / 2])
                .collect();
            vec![
                crate::Raster::new(self.document.width(), self.document.height(), pixels)
                    .expect("mask dimensions"),
            ]
        } else {
            self.document
                .layers()
                .iter()
                .map(|l| l.raster().clone())
                .collect()
        };
        self.transform_preview.layers = rasters
            .iter()
            .enumerate()
            .map(|(i, raster)| {
                let w = raster.width() as usize;
                let h = raster.height() as usize;
                let mut padded = vec![0; (w + 2) * (h + 2) * 4];
                for y in 0..h {
                    let start = ((y + 1) * (w + 2) + 1) * 4;
                    padded[start..start + w * 4]
                        .copy_from_slice(&raster.pixels()[y * w * 4..(y + 1) * w * 4]);
                }
                ui.ctx().load_texture(
                    format!("transform-layer-{i}"),
                    egui::ColorImage::from_rgba_unmultiplied([w + 2, h + 2], &padded),
                    egui::TextureOptions::LINEAR,
                )
            })
            .collect();
    }
}
