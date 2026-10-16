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
use crate::tab::{DocTab, Moving, PageTexture, Rastered, Resize};
use crate::theme::Palette;
use crate::worker::{ObjectCommand, PictureEdit, Request};

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

/// The eight handles of a frame: corners nw, ne, se, sw, then edges n, e,
/// s, w.
fn handles(frame: Rect) -> [egui::Pos2; 8] {
    [
        frame.left_top(),
        frame.right_top(),
        frame.right_bottom(),
        frame.left_bottom(),
        frame.center_top(),
        frame.right_center(),
        frame.center_bottom(),
        frame.left_center(),
    ]
}

/// The pointer shape over a handle.
fn handle_cursor(handle: usize) -> egui::CursorIcon {
    match handle {
        0 | 2 => egui::CursorIcon::ResizeNwSe,
        1 | 3 => egui::CursorIcon::ResizeNeSw,
        4 | 6 => egui::CursorIcon::ResizeVertical,
        _ => egui::CursorIcon::ResizeHorizontal,
    }
}

/// The frame a resize gives: the dragged edges follow the pointer, the
/// others stay; unless Shift frees them, the proportions are kept.
fn resized(resize: &Resize) -> Rect {
    let delta = resize.pointer - resize.origin;
    let frame = resize.frame;
    let (mut left, mut top, mut right, mut bottom) =
        (frame.left(), frame.top(), frame.right(), frame.bottom());
    let (west, north, east, south) = match resize.handle {
        0 => (true, true, false, false),
        1 => (false, true, true, false),
        2 => (false, false, true, true),
        3 => (true, false, false, true),
        4 => (false, true, false, false),
        5 => (false, false, true, false),
        6 => (false, false, false, true),
        _ => (true, false, false, false),
    };
    if west {
        left = (left + delta.x).min(right - 4.0);
    }
    if east {
        right = (right + delta.x).max(left + 4.0);
    }
    if north {
        top = (top + delta.y).min(bottom - 4.0);
    }
    if south {
        bottom = (bottom + delta.y).max(top + 4.0);
    }
    if resize.crop {
        // A crop stays inside the picture.
        return Rect::from_min_max(
            pos2(left.max(frame.left()), top.max(frame.top())),
            pos2(right.min(frame.right()), bottom.min(frame.bottom())),
        );
    }
    if !resize.free && frame.width() > 0.0 && frame.height() > 0.0 {
        // The dragged side decides (the larger change, at a corner); the
        // other follows the aspect.
        let wide = (right - left) / frame.width();
        let high = (bottom - top) / frame.height();
        let scale = match resize.handle {
            4 | 6 => high,
            5 | 7 => wide,
            _ => wide.max(high),
        };
        let (width, height) = (frame.width() * scale, frame.height() * scale);
        if west {
            left = right - width;
        } else {
            right = left + width;
        }
        if north {
            top = bottom - height;
        } else {
            bottom = top + height;
        }
    }
    Rect::from_min_max(pos2(left, top), pos2(right, bottom))
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
                // Where the pages are on screen, for the rulers.
                let clip = ui.clip_rect();
                tab.view.page_screens = rects
                    .iter()
                    .map(|rect| {
                        let screen = rect.translate(offset);
                        screen.intersects(clip).then_some(screen)
                    })
                    .collect();
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
                // Editing a header or footer: the body is dimmed and the
                // header's (or footer's) edge marked.
                if let Some(view) = tab.cursor.as_ref().and_then(|c| c.header_footer.as_ref()) {
                    for (index, rect) in rects.iter().enumerate() {
                        let Some((top, bottom)) = view.body.get(index).copied() else {
                            continue;
                        };
                        let screen = rect.translate(offset);
                        if !screen.intersects(ui.clip_rect()) {
                            continue;
                        }
                        let z = tab.zoom;
                        #[allow(clippy::cast_possible_truncation)]
                        let (top, bottom) = (
                            screen.top() + top as f32 * z,
                            screen.top() + bottom as f32 * z,
                        );
                        painter.rect_filled(
                            Rect::from_x_y_ranges(screen.x_range(), top..=bottom),
                            0.0,
                            Color32::from_white_alpha(150),
                        );
                        let edge = if view.header { top } else { bottom };
                        let dash = Stroke::new(1.0, crate::theme::ACCENT);
                        let mut x = screen.left();
                        while x < screen.right() {
                            painter.line_segment(
                                [pos2(x, edge), pos2((x + 5.0).min(screen.right()), edge)],
                                dash,
                            );
                            x += 9.0;
                        }
                    }
                }
                // A selected drawing: its frame and handles, or the frame a
                // resize is dragging out.
                let object_frame = tab.cursor.as_ref().and_then(|cursor| {
                    let object = cursor.object.as_ref()?;
                    let rect = object.rects.first()?;
                    to_screen(rect.page, rect.x, rect.y, rect.width, rect.height)
                        .map(|frame| (frame, (object.width, object.height)))
                });
                let floating = tab
                    .cursor
                    .as_ref()
                    .and_then(|cursor| cursor.object.as_ref())
                    .is_some_and(|object| object.wrap != crate::worker::WrapStyle::Inline);
                // A drawing being dragged: where it would go.
                if let Some(moving) = &tab.view.moving {
                    let ghost = moving.frame.translate(moving.pointer - moving.origin);
                    painter.rect_stroke(
                        ghost,
                        0.0,
                        Stroke::new(1.5, crate::theme::ACCENT),
                        egui::StrokeKind::Outside,
                    );
                    if !moving.floating {
                        // In the text it goes where a click there would put
                        // the caret.
                        painter.line_segment(
                            [
                                moving.pointer - vec2(0.0, 10.0),
                                moving.pointer + vec2(0.0, 10.0),
                            ],
                            Stroke::new(2.0, crate::theme::ACCENT),
                        );
                    }
                }
                if object_frame.is_none() {
                    tab.view.crop = false;
                }
                if let Some((frame, _)) = object_frame {
                    let accent = crate::theme::ACCENT;
                    let shown = tab.view.resize.as_ref().map_or(frame, resized);
                    // Cropping: what goes is shaded.
                    if tab.view.crop && shown != frame {
                        let shade = Color32::from_black_alpha(110);
                        for part in [
                            Rect::from_min_max(frame.left_top(), pos2(frame.right(), shown.top())),
                            Rect::from_min_max(
                                pos2(frame.left(), shown.bottom()),
                                frame.right_bottom(),
                            ),
                            Rect::from_min_max(
                                pos2(frame.left(), shown.top()),
                                pos2(shown.left(), shown.bottom()),
                            ),
                            Rect::from_min_max(
                                pos2(shown.right(), shown.top()),
                                pos2(frame.right(), shown.bottom()),
                            ),
                        ] {
                            painter.rect_filled(part, 0.0, shade);
                        }
                    }
                    painter.rect_stroke(
                        shown,
                        0.0,
                        Stroke::new(1.5, accent),
                        egui::StrokeKind::Outside,
                    );
                    for handle in handles(shown) {
                        if tab.view.crop {
                            // Crop handles are black bars, as in Word.
                            let bar = Rect::from_center_size(handle, vec2(9.0, 9.0));
                            painter.rect_filled(bar, 0.0, Color32::BLACK);
                            painter.rect_stroke(
                                bar,
                                0.0,
                                Stroke::new(1.0, Color32::WHITE),
                                egui::StrokeKind::Inside,
                            );
                            continue;
                        }
                        let square = Rect::from_center_size(handle, vec2(7.0, 7.0));
                        painter.rect_filled(square, 1.0, Color32::WHITE);
                        painter.rect_stroke(
                            square,
                            1.0,
                            Stroke::new(1.0, accent),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
                let on_handle = |pos: egui::Pos2| {
                    let (frame, size) = object_frame?;
                    handles(frame)
                        .iter()
                        .position(|handle| handle.distance(pos) <= 6.0)
                        .map(|index| (index, frame, size))
                };
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
                // A handle of the selected drawing resizes it. The drag is
                // known only once the pointer has moved, so the handle is
                // looked for where the button went down.
                let pressed = response.interact_pointer_pos();
                if response.drag_started() && !tab.view.crop {
                    // Dragging the selected drawing's body moves it.
                    let origin = ui.input(|input| input.pointer.press_origin());
                    if let (Some(origin), Some((frame, _))) = (origin, object_frame) {
                        if frame.contains(origin) && on_handle(origin).is_none() {
                            tab.view.moving = Some(Moving {
                                frame,
                                origin,
                                pointer: pressed.unwrap_or(origin),
                                floating,
                            });
                        }
                    }
                }
                if response.drag_started() && tab.view.moving.is_none() {
                    let origin = ui.input(|input| input.pointer.press_origin());
                    if let Some((handle, frame, size)) = origin.and_then(on_handle) {
                        if let Some(origin) = origin {
                            tab.view.resize = Some(Resize {
                                handle,
                                frame,
                                pointer: pressed.unwrap_or(origin),
                                origin,
                                size,
                                free: shift,
                                crop: tab.view.crop,
                            });
                        }
                    }
                }
                if let Some(resize) = tab.view.resize.as_mut() {
                    if let Some(pos) = pressed {
                        resize.pointer = pos;
                    }
                    resize.free = shift;
                    ctx.set_cursor_icon(handle_cursor(resize.handle));
                    if response.drag_stopped() {
                        let resize = *resize;
                        tab.view.resize = None;
                        let frame = resized(&resize);
                        if resize.crop && resize.frame.width() > 0.0 && resize.frame.height() > 0.0
                        {
                            let old = resize.frame;
                            let sides = [
                                (frame.left() - old.left()) / old.width(),
                                (frame.top() - old.top()) / old.height(),
                                (old.right() - frame.right()) / old.width(),
                                (old.bottom() - frame.bottom()) / old.height(),
                            ];
                            if sides.iter().any(|side| *side > 0.002) {
                                tab.send(Request::Object(ObjectCommand::Edit(PictureEdit::Crop {
                                    sides,
                                })));
                            }
                        } else if resize.frame.width() > 0.0 && resize.frame.height() > 0.0 {
                            #[allow(clippy::cast_possible_truncation)]
                            let scale = |emu: i64, to: f32, from: f32| {
                                (emu as f64 * f64::from(to) / f64::from(from)).round() as i64
                            };
                            tab.send(Request::Object(ObjectCommand::Resize {
                                width: scale(resize.size.0, frame.width(), resize.frame.width()),
                                height: scale(resize.size.1, frame.height(), resize.frame.height()),
                            }));
                        }
                    }
                } else if let Some(moving) = tab.view.moving.as_mut() {
                    if let Some(pos) = pressed {
                        moving.pointer = pos;
                    }
                    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                    if response.drag_stopped() {
                        let moving = *moving;
                        tab.view.moving = None;
                        let delta = moving.pointer - moving.origin;
                        if delta.length() > 2.0 {
                            if moving.floating {
                                let z = f64::from(tab.zoom);
                                tab.send(Request::Object(ObjectCommand::Move {
                                    dx: f64::from(delta.x) / z,
                                    dy: f64::from(delta.y) / z,
                                }));
                            } else if let Some((page, x, y)) = hit(moving.pointer) {
                                tab.send(Request::Object(ObjectCommand::MoveTo { page, x, y }));
                            }
                        }
                    }
                } else if response.double_clicked() {
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
                if response.hovered() && tab.view.resize.is_none() {
                    let pointer = ui.input(|input| input.pointer.hover_pos());
                    let over_handle = pointer.and_then(on_handle).map(|(handle, _, _)| handle);
                    let over_object = pointer
                        .zip(object_frame)
                        .is_some_and(|(pos, (frame, _))| frame.contains(pos));
                    ctx.set_cursor_icon(match over_handle {
                        Some(handle) => handle_cursor(handle),
                        None if over_object => egui::CursorIcon::Grab,
                        None => egui::CursorIcon::Text,
                    });
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
