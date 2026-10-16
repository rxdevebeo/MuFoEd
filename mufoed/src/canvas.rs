//! The desk: pages in a column, the caret and the selection over them.
//!
//! Pages arrive laid out. Only the pages in view (and one either side) are
//! painted to SVG and rasterized, on the thread pool, at the zoom and the
//! screen's pixel density; bitmaps of pages far out of view are dropped. While
//! a new bitmap is being made the old one is stretched, so zooming or editing
//! never shows a blank page, and a re-laid page whose SVG did not change keeps
//! its bitmap.

use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

use eframe::egui::{self, pos2, vec2, Color32, Rect, Sense, Stroke};

use crate::raster;
use crate::tab::{DocTab, PageTexture, Rastered};
use crate::theme::Palette;
use crate::worker::Request;

/// Painting for the screen: the rasterizer has the faces, so they are not
/// embedded in each page.
fn paint_options() -> &'static strict_ooxml::RenderOptions {
    static OPTIONS: OnceLock<strict_ooxml::RenderOptions> = OnceLock::new();
    OPTIONS.get_or_init(|| strict_ooxml::RenderOptions::default().embed_fonts(false))
}

/// Gap between pages and around the column, in points.
const GAP: f32 = 18.0;
/// How long the zoom must hold still before pages are re-rasterized.
const ZOOM_SETTLE: f64 = 0.18;
/// At most this many bitmaps in flight per tab.
const MAX_PENDING: usize = 4;

/// Where each page sits in the column, in content coordinates.
pub(crate) fn page_rects(tab: &DocTab) -> (Vec<Rect>, egui::Vec2) {
    let zoom = tab.zoom;
    #[allow(clippy::cast_possible_truncation)]
    let widest = tab
        .pages
        .iter()
        .map(|page| page.width as f32 * zoom)
        .fold(0.0_f32, f32::max);
    let column = widest + GAP * 2.0;
    let mut y = GAP;
    let mut rects = Vec::with_capacity(tab.pages.len());
    for page in &tab.pages {
        #[allow(clippy::cast_possible_truncation)]
        let size = vec2(page.width as f32 * zoom, page.height as f32 * zoom);
        rects.push(Rect::from_min_size(pos2((column - size.x) / 2.0, y), size));
        y += size.y + GAP;
    }
    (rects, vec2(column, y))
}

fn receive_bitmaps(ctx: &egui::Context, tab: &mut DocTab) {
    while let Ok(Rastered {
        index,
        generation,
        svg,
        scale,
        bitmap,
    }) = tab.view.raster_rx.try_recv()
    {
        tab.view.pending.remove(&(index, generation, scale));
        let current = tab.pages.get(index).map(|page| page.generation);
        if current != Some(generation) {
            continue;
        }
        let Some(bitmap) = bitmap else {
            // The new layout painted the same SVG: the bitmap stands.
            if let Some(texture) = tab.view.textures.get_mut(&index) {
                if texture.svg == svg && texture.scale == scale {
                    texture.generation = generation;
                }
            }
            continue;
        };
        let Ok(bitmap) = bitmap else {
            continue;
        };
        let image =
            egui::ColorImage::from_rgba_premultiplied([bitmap.width, bitmap.height], &bitmap.rgba);
        let texture = ctx.load_texture(
            format!("page-{}-{index}", tab.id),
            image,
            egui::TextureOptions::LINEAR,
        );
        tab.view.textures.insert(
            index,
            PageTexture {
                generation,
                svg,
                scale,
                texture,
            },
        );
    }
}

fn request_bitmap(ctx: &egui::Context, tab: &mut DocTab, index: usize, scale: f32, key: u32) {
    let Some(page) = tab.pages.get(index) else {
        return;
    };
    let job = (index, page.generation, key);
    if tab.view.pending.contains(&job) || tab.view.pending.len() >= MAX_PENDING {
        return;
    }
    tab.view.pending.insert(job);
    let page = std::sync::Arc::clone(page);
    // The SVG the bitmap at this scale was made from, if there is one.
    let known = tab
        .view
        .textures
        .get(&index)
        .filter(|texture| texture.scale == key)
        .map(|texture| texture.svg);
    let sender = tab.view.raster_tx.clone();
    let ctx = ctx.clone();
    rayon::spawn(move || {
        let svg = strict_ooxml::paint_page(&page.placed, paint_options());
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        svg.hash(&mut hasher);
        let hash = hasher.finish();
        let bitmap = (known != Some(hash)).then(|| raster::rasterize(&svg, scale));
        let _ = sender.send(Rastered {
            index,
            generation: page.generation,
            svg: hash,
            scale: key,
            bitmap,
        });
        ctx.request_repaint();
    });
}

/// Draws the desk for `tab`; requests go to its worker.
#[allow(clippy::too_many_lines)]
pub(crate) fn show(ui: &mut egui::Ui, tab: &mut DocTab, palette: Palette, keyboard: bool) {
    let ctx = ui.ctx().clone();
    receive_bitmaps(&ctx, tab);
    let now = ui.input(|input| input.time);
    let (rects, content) = page_rects(tab);
    let scale = tab.zoom * ctx.pixels_per_point();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let key = (scale * 64.0).round().max(1.0) as u32;
    let settled = now - tab.view.zoom_changed > ZOOM_SETTLE;
    egui::Frame::new().fill(palette.desk).show(ui, |ui| {
        let scroll = egui::ScrollArea::both()
            .id_salt(("desk", tab.id))
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                tab.view.viewport = viewport.size();
                let size = vec2(
                    content.x.max(viewport.width()),
                    content.y.max(viewport.height()),
                );
                let (area, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
                // Centre the column when the window is wider than the pages.
                let center = vec2(((size.x - content.x) / 2.0).max(0.0), 0.0);
                let offset = area.min.to_vec2() + center;
                let painter = ui.painter_at(area);
                let mut visible = Vec::new();
                for (index, rect) in rects.iter().enumerate() {
                    let screen = rect.translate(offset);
                    let local = rect.translate(center);
                    let near = local.expand2(vec2(0.0, rect.height())).intersects(viewport);
                    if !near {
                        continue;
                    }
                    if local.intersects(viewport) {
                        visible.push(index);
                    }
                    painter.rect_filled(
                        screen.translate(vec2(1.5, 2.5)),
                        1.0,
                        Color32::from_black_alpha(40),
                    );
                    painter.rect_filled(screen, 0.0, Color32::WHITE);
                    let generation = tab.pages.get(index).map(|page| page.generation);
                    let texture = tab.view.textures.get(&index);
                    if let Some(texture) = texture {
                        painter.image(
                            texture.texture.id(),
                            screen,
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    let fresh =
                        texture.is_some_and(|t| Some(t.generation) == generation && t.scale == key);
                    let stale_content = texture.is_some_and(|t| Some(t.generation) != generation);
                    let missing = texture.is_none();
                    if !fresh && (settled || missing || stale_content) {
                        request_bitmap(&ctx, tab, index, scale, key);
                    }
                    if missing {
                        // The page is laid out but not drawn yet.
                        let size = (screen.width() / 6.0).clamp(12.0, 36.0);
                        egui::Spinner::new()
                            .size(size)
                            .color(Color32::from_gray(160))
                            .paint_at(
                                ui,
                                Rect::from_center_size(screen.center(), vec2(size, size)),
                            );
                    }
                }
                // Bitmaps of pages far out of view go.
                if let (Some(first), Some(last)) =
                    (visible.first().copied(), visible.last().copied())
                {
                    tab.view
                        .textures
                        .retain(|index, _| *index + 3 >= first && *index <= last + 3);
                    tab.view.current_page = visible
                        .iter()
                        .copied()
                        .max_by(|a, b| {
                            let overlap = |i: usize| {
                                rects.get(i).map_or(0.0, |r| {
                                    r.translate(center).intersect(viewport).height()
                                })
                            };
                            overlap(*a).total_cmp(&overlap(*b))
                        })
                        .unwrap_or(first);
                }
                let to_screen = |page: usize, x: f64, y: f64, w: f64, h: f64| -> Option<Rect> {
                    let rect = rects.get(page)?;
                    let z = tab.zoom;
                    #[allow(clippy::cast_possible_truncation)]
                    let r = Rect::from_min_size(
                        pos2(rect.left() + x as f32 * z, rect.top() + y as f32 * z),
                        vec2(w as f32 * z, h as f32 * z),
                    );
                    Some(r.translate(offset))
                };
                if let Some(cursor) = &tab.cursor {
                    for rect in &cursor.selection {
                        if let Some(screen) =
                            to_screen(rect.page, rect.x, rect.y, rect.width, rect.height)
                        {
                            painter.rect_filled(screen, 0.0, palette.selection);
                        }
                    }
                    let blink = ((now - tab.view.caret_moved) * 1.6) % 2.0 < 1.25;
                    let has_selection = !cursor.selection.is_empty();
                    for rect in &cursor.carets {
                        if let Some(screen) = to_screen(rect.page, rect.x, rect.y, 0.0, rect.height)
                        {
                            if blink && !has_selection && keyboard {
                                painter.line_segment(
                                    [screen.left_top(), screen.left_bottom()],
                                    Stroke::new(1.6, palette.caret),
                                );
                            }
                            if tab.view.scroll_to_caret {
                                ui.scroll_to_rect(screen.expand(40.0), None);
                            }
                        }
                    }
                    if tab.view.scroll_to_caret {
                        tab.view.scroll_to_caret = false;
                        tab.view.caret_moved = now;
                    }
                    ctx.request_repaint_after(std::time::Duration::from_millis(400));
                }
                if let Some(page) = tab.view.scroll_to_page.take() {
                    if let Some(rect) = rects.get(page) {
                        ui.scroll_to_rect(rect.translate(offset), Some(egui::Align::TOP));
                    }
                }
                // Mouse: click, shift-click and drag move the caret.
                let hit = |pos: egui::Pos2| -> Option<(usize, f64, f64)> {
                    let local = pos - offset;
                    let local = pos2(local.x, local.y);
                    let (index, rect) = rects.iter().enumerate().min_by(|(_, a), (_, b)| {
                        a.distance_to_pos(local)
                            .total_cmp(&b.distance_to_pos(local))
                    })?;
                    let z = f64::from(tab.zoom);
                    Some((
                        index,
                        f64::from(local.x - rect.left()) / z,
                        f64::from(local.y - rect.top()) / z,
                    ))
                };
                let shift = ui.input(|input| input.modifiers.shift);
                let column = ui.input(|input| input.modifiers.command);
                if response.double_clicked() {
                    if let Some((page, x, y)) = response.interact_pointer_pos().and_then(hit) {
                        tab.send(Request::SelectWord { page, x, y });
                    }
                } else if response.drag_started() || response.clicked() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        if let Some((page, x, y)) = hit(pos) {
                            tab.send(Request::Click {
                                page,
                                x,
                                y,
                                extend: shift,
                                column,
                            });
                            tab.view.last_drag = Some(pos);
                            tab.view.caret_moved = now;
                        }
                    }
                } else if response.dragged() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        if tab
                            .view
                            .last_drag
                            .is_none_or(|last| last.distance(pos) > 1.5)
                        {
                            if let Some((page, x, y)) = hit(pos) {
                                tab.send(Request::Click {
                                    page,
                                    x,
                                    y,
                                    extend: true,
                                    column: false,
                                });
                            }
                            tab.view.last_drag = Some(pos);
                        }
                    }
                }
                if response.drag_stopped() {
                    tab.view.last_drag = None;
                }
                if response.hovered() {
                    ctx.set_cursor_icon(egui::CursorIcon::Text);
                    let zoom = ui.input(egui::InputState::zoom_delta);
                    if (zoom - 1.0).abs() > f32::EPSILON {
                        tab.zoom = (tab.zoom * zoom).clamp(0.25, 5.0);
                        tab.view.zoom_changed = now;
                    }
                }
            });
        let _ = scroll;
    });
    if !settled {
        ctx.request_repaint_after(std::time::Duration::from_millis(60));
    }
}
