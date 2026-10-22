//! Image clipboard commands; shared masks also serve as pixel selections.
use super::ImageEditor;
use crate::{Document, EditError, PixelClipboard};

struct PastedObject {
    before: Document,
    expected: Document,
    pixels: PixelClipboard,
    at: [i32; 2],
    new_layer: bool,
    bounds: [u32; 4],
    mask_target: crate::MaskTarget,
    drag: Option<([f32; 2], [i32; 2])>,
}

#[derive(Default)]
pub(super) struct ClipboardUi {
    pixels: Option<PixelClipboard>,
    at: [i32; 2],
    merged: bool,
    floating: Option<PastedObject>,
    paste_handled: bool,
    internal_only: bool,
    #[cfg(all(feature = "native-clipboard", not(test)))]
    system: Option<arboard::Clipboard>,
}
fn fragment_bounds(pixels: &PixelClipboard) -> [u32; 4] {
    let r = pixels.raster();
    let mut b = [r.width(), r.height(), 0, 0];
    for (i, pixel) in r.pixels().chunks_exact(4).enumerate() {
        if pixel[3] == 0 {
            continue;
        }
        let x = u32::try_from(i % r.width() as usize).expect("fragment width");
        let y = u32::try_from(i / r.width() as usize).expect("fragment height");
        b = [b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1)];
    }
    if b[2] == 0 {
        [0, 0, r.width(), r.height()]
    } else {
        b
    }
}
fn select_pasted_bounds(
    doc: &mut Document,
    pixels: &PixelClipboard,
    at: [i32; 2],
) -> Result<(), EditError> {
    let left = i64::from(at[0]).max(0).min(i64::from(doc.width()));
    let top = i64::from(at[1]).max(0).min(i64::from(doc.height()));
    let right =
        (i64::from(at[0]) + i64::from(pixels.raster().width())).clamp(left, i64::from(doc.width()));
    let bottom = (i64::from(at[1]) + i64::from(pixels.raster().height()))
        .clamp(top, i64::from(doc.height()));
    let mut area = vec![0; doc.mask().len()];
    for row in top..bottom {
        let offset = usize::try_from(row).expect("canvas row") * doc.width() as usize;
        for x in left..right {
            let sx = u32::try_from(x - i64::from(at[0])).expect("clipped");
            let sy = u32::try_from(row - i64::from(at[1])).expect("clipped");
            if pixels.raster().pixel(sx, sy).is_some_and(|p| p[3] > 0) {
                area[offset + usize::try_from(x).expect("clipped")] = 255;
            }
        }
    }
    doc.set_selection(area)
}
impl ClipboardUi {
    pub(super) fn reset_document_position(&mut self) {
        self.floating = None;
        self.at = [0; 2];
        self.paste_handled = false;
    }
    #[cfg(all(feature = "native-clipboard", not(test)))]
    fn system(&mut self) -> Result<&mut arboard::Clipboard, arboard::Error> {
        if self.system.is_none() {
            self.system = Some(arboard::Clipboard::new()?);
        }
        Ok(self.system.as_mut().expect("initialized"))
    }
    #[allow(clippy::unnecessary_wraps, clippy::unused_self)] // Same fallible boundary on native and embedded hosts.
    fn publish(&mut self) -> Result<(), String> {
        #[cfg(all(feature = "native-clipboard", not(test)))]
        {
            let raster = self.pixels.as_ref().expect("copied").raster();
            let image = arboard::ImageData {
                width: raster.width() as usize,
                height: raster.height() as usize,
                bytes: std::borrow::Cow::Owned(raster.pixels().to_vec()),
            };
            self.system()
                .and_then(|s| s.set_image(image))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    #[allow(clippy::unnecessary_wraps, clippy::unused_self)] // Native clipboard I/O is feature-gated.
    fn read_external(&mut self) -> Result<(), EditError> {
        if self.internal_only {
            return Ok(());
        }
        #[cfg(all(feature = "native-clipboard", not(test)))]
        {
            // The internal buffer remains usable on hosts without a native clipboard.
            let Ok(system) = self.system() else {
                return Ok(());
            };
            let image = system.get_image().map_err(|e| {
                EditError::Invalid(format!("В буфере обмена нет доступного изображения: {e}"))
            })?;
            let raster = crate::Raster::new(
                u32::try_from(image.width)
                    .map_err(|_| EditError::Invalid("Слишком широкое изображение".into()))?,
                u32::try_from(image.height)
                    .map_err(|_| EditError::Invalid("Слишком высокое изображение".into()))?,
                image.bytes.into_owned(),
            )?;
            if self.pixels.as_ref().is_none_or(|p| p.raster() != &raster) {
                self.pixels = Some(PixelClipboard::from_raster(raster));
                self.at = [0; 2];
            }
        }
        Ok(())
    }
}
impl ImageEditor {
    /// Copy the selected content type: image pixels (retaining alpha) or mask coverage.
    pub fn copy_pixels(&mut self, merged: bool) -> bool {
        self.finish_manual_gesture();
        let copied = if self.edit_target() == super::EditTarget::Mask {
            self.document.copy_target_mask(self.mask_target())
        } else {
            self.document.copy_selection(merged)
        };
        match copied {
            Ok(pixels) => {
                self.clipboard_ui.at = pixels
                    .origin()
                    .map(|v| i32::try_from(v).expect("bounded canvas"));
                self.status = format!(
                    "Скопировано {}×{} · {}",
                    pixels.raster().width(),
                    pixels.raster().height(),
                    if pixels.is_mask() {
                        "маска"
                    } else if merged {
                        "видимые слои"
                    } else {
                        "текущий слой"
                    }
                );
                self.clipboard_ui.pixels = Some(pixels);
                self.clipboard_ui.internal_only = false;
                if let Err(e) = self.clipboard_ui.publish() {
                    self.clipboard_ui.internal_only = true;
                    self.status =
                        format!("Фрагмент во внутреннем буфере; системный буфер недоступен: {e}");
                }
            }
            Err(e) => {
                self.status = e.to_string();
                return false;
            }
        }
        true
    }
    pub(super) fn cut_pixels(&mut self) {
        if self.copy_pixels(false) {
            self.delete_selected();
        }
    }
    pub(super) fn delete_selected(&mut self) {
        if self.has_pasted_object() {
            self.restore_history(false);
            return;
        }
        let mask = self.edit_target() == super::EditTarget::Mask;
        let target = self.mask_target();
        self.edit(|d| {
            if mask {
                d.with_mask_target(target, |d| d.delete_area(true))
            } else {
                d.delete_area(false)
            }
        });
    }
    /// Paste the system/internal fragment and activate Hand for placement.
    /// Insertion and subsequent placement share one undo step.
    pub fn paste_clipboard(&mut self, new_layer: bool) {
        self.finish_manual_gesture();
        if let Err(e) = self.clipboard_ui.read_external() {
            self.status = e.to_string();
            return;
        }
        let Some(pixels) = self.clipboard_ui.pixels.clone() else {
            self.status = "В буфере нет изображения. Сначала скопируйте область".into();
            return;
        };
        let pixels = pixels.for_mask(self.edit_target() == super::EditTarget::Mask);
        let at = self.clipboard_ui.at;
        let mask_target = if pixels.is_mask() {
            self.mask_target()
        } else {
            crate::MaskTarget::Inpaint
        };
        self.finish_pasted_object();
        let before = self.document.clone();
        let mut placed = before.clone();
        if let Err(e) = placed
            .with_mask_target(mask_target, |d| d.paste_pixels(&pixels, at, new_layer))
            .and_then(|()| select_pasted_bounds(&mut placed, &pixels, at))
        {
            self.status = e.to_string();
            return;
        }
        self.select_manual_tool(super::ManualTool::Hand);
        self.history.checkpoint(&before);
        self.document = placed;
        self.changed = true;
        self.unsaved = true;
        self.manual_ui.thumbnails.clear();
        self.clear_segmentation_points();
        if pixels.is_mask() {
            if mask_target == crate::MaskTarget::Inpaint {
                self.generation_ui.show_mask = true;
            } else {
                self.manual_ui.show_layer_mask = true;
            }
        }
        let bounds = fragment_bounds(&pixels);
        self.clipboard_ui.floating = Some(PastedObject {
            before,
            expected: self.document.clone(),
            pixels,
            at,
            new_layer,
            bounds,
            mask_target,
            drag: None,
        });
        self.status =
            "Рука: перетащите вставленный объект · Enter — закрепить · Esc — отменить вставку"
                .into();
    }
    pub(super) fn finish_pasted_object(&mut self) {
        if self.clipboard_ui.floating.take().is_some() {
            if self.floating_transform() {
                self.cancel_layer_transform();
            }
            self.refresh_saved_state();
        }
    }
    pub(super) fn pasted_transform_source(&self) -> Result<Document, EditError> {
        let p = self
            .clipboard_ui
            .floating
            .as_ref()
            .filter(|_| self.has_pasted_object())
            .ok_or_else(|| EditError::Invalid("Вставка больше не активна".into()))?;
        let mut doc = Document::new(self.document.width(), self.document.height())?;
        doc.paste_pixels(&p.pixels.for_mask(false), p.at, false)?;
        doc.set_layer("Вставленный фрагмент".into(), true, 255);
        Ok(doc)
    }
    pub(super) fn pasted_transform_document(
        &self,
        raster: &crate::Raster,
    ) -> Result<Document, EditError> {
        let p = self
            .clipboard_ui
            .floating
            .as_ref()
            .filter(|_| self.has_pasted_object())
            .ok_or_else(|| EditError::Invalid("Вставка больше не активна".into()))?;
        let pixels = PixelClipboard::from_raster(raster.clone()).for_mask(p.pixels.is_mask());
        let mut doc = p.before.clone();
        doc.with_mask_target(p.mask_target, |d| {
            d.paste_pixels(&pixels, [0; 2], p.new_layer)
        })?;
        select_pasted_bounds(&mut doc, &pixels, [0; 2])?;
        Ok(doc)
    }
    pub(super) fn apply_pasted_transform(
        &mut self,
        raster: crate::Raster,
    ) -> Result<(), EditError> {
        let doc = self.pasted_transform_document(&raster)?;
        let p = self
            .clipboard_ui
            .floating
            .as_mut()
            .expect("validated paste");
        p.pixels = PixelClipboard::from_raster(raster).for_mask(p.pixels.is_mask());
        p.at = [0; 2];
        p.bounds = fragment_bounds(&p.pixels);
        p.drag = None;
        p.expected = doc.clone();
        self.document = doc;
        self.changed = true;
        self.unsaved = true;
        self.manual_ui.thumbnails.clear();
        Ok(())
    }
    pub(super) fn pause_pasted_drag(&mut self) {
        if let Some(p) = self.clipboard_ui.floating.as_mut() {
            p.drag = None;
        }
    }
    pub(super) fn has_pasted_object(&self) -> bool {
        self.clipboard_ui
            .floating
            .as_ref()
            .is_some_and(|p| p.expected == self.document)
    }
    // Recompose against the pre-paste document: moving a current-layer fragment
    // must restore its background, never translate the existing layer pixels.
    pub(super) fn move_pasted_object(&mut self, at: [i32; 2]) {
        let Some(p) = self.clipboard_ui.floating.as_mut() else {
            return;
        };
        if p.expected != self.document {
            self.finish_pasted_object();
            return;
        }
        let mut placed = p.before.clone();
        match placed
            .with_mask_target(p.mask_target, |d| {
                d.paste_pixels(&p.pixels, at, p.new_layer)
            })
            .and_then(|()| select_pasted_bounds(&mut placed, &p.pixels, at))
        {
            Ok(()) => {
                self.document = placed;
                p.expected = self.document.clone();
                p.at = at;
                self.clipboard_ui.at = at;
                self.changed = true;
                self.unsaved = true;
                self.manual_ui.thumbnails.clear();
            }
            Err(e) => self.status = e.to_string(),
        }
    }
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    pub(super) fn pasted_canvas(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        point: Option<[f32; 2]>,
    ) {
        if !self.has_pasted_object() {
            self.finish_pasted_object();
            return;
        }
        let p = self.clipboard_ui.floating.as_mut().expect("checked");
        let size = [
            p.pixels.raster().width() as f32,
            p.pixels.raster().height() as f32,
        ];
        let rect = egui::Rect::from_min_size(
            response.rect.min
                + egui::vec2(
                    p.at[0] as f32 + p.bounds[0] as f32,
                    p.at[1] as f32 + p.bounds[1] as f32,
                ) * self.zoom,
            egui::vec2(
                (p.bounds[2] - p.bounds[0]) as f32,
                (p.bounds[3] - p.bounds[1]) as f32,
            ) * self.zoom,
        );
        ui.painter()
            .with_clip_rect(response.rect.intersect(ui.clip_rect()))
            .rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.5, ui.visuals().selection.stroke.color),
                egui::StrokeKind::Inside,
            );
        let pressed = ui.input(|i| i.pointer.primary_pressed());
        let down = ui.input(|i| i.pointer.primary_down());
        if pressed && response.contains_pointer() {
            if let Some(point) = point {
                let local = [point[0] - p.at[0] as f32, point[1] - p.at[1] as f32];
                if local[0] >= p.bounds[0] as f32
                    && local[1] >= p.bounds[1] as f32
                    && local[0] < p.bounds[2] as f32
                    && local[1] < p.bounds[3] as f32
                {
                    p.drag = Some((point, p.at));
                }
            }
        }
        if let Some((start, origin)) = p.drag {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            if let Some(point) = point {
                let at = [
                    (origin[0] + (point[0] - start[0]).round() as i32).clamp(
                        1 - size[0] as i32,
                        i32::try_from(self.document.width()).expect("bounded canvas") - 1,
                    ),
                    (origin[1] + (point[1] - start[1]).round() as i32).clamp(
                        1 - size[1] as i32,
                        i32::try_from(self.document.height()).expect("bounded canvas") - 1,
                    ),
                ];
                if at != p.at {
                    self.move_pasted_object(at);
                    ui.ctx().request_repaint();
                }
            }
            if !down {
                if let Some(p) = self.clipboard_ui.floating.as_mut() {
                    p.drag = None;
                }
            }
        }
    }
    pub(super) fn select_all_pixels(&mut self) {
        self.edit(|d| d.set_selection(vec![255; d.mask().len()]));
        self.changed = true;
    }
    pub(super) fn clipboard_shortcuts(&mut self, ui: &egui::Ui) {
        if self.has_pasted_object() {
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.finish_pasted_object();
            } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.restore_history(false);
            }
        }
        let mods = ui.input(|i| i.modifiers);
        if ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Cut)))
            || (mods.command && ui.input(|i| i.key_pressed(egui::Key::X)))
        {
            self.cut_pixels();
        }
        if !mods.command && ui.input(|i| i.key_pressed(egui::Key::Delete)) {
            self.delete_selected();
        }
        let copy = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)));
        if copy || (mods.command && ui.input(|i| i.key_pressed(egui::Key::C))) {
            self.copy_pixels(self.clipboard_ui.merged || mods.shift);
        }
        let paste = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Paste(_))))
            || (mods.command && ui.input(|i| i.key_pressed(egui::Key::V)));
        let released = ui.input(|i| i.key_released(egui::Key::V));
        // egui-winit consumes Ctrl+V key-down for its text clipboard. For images
        // it emits no Paste event; key-up is still delivered. Do not paste twice.
        if paste || (released && mods.command && !self.clipboard_ui.paste_handled) {
            self.paste_clipboard(!mods.shift);
            self.clipboard_ui.paste_handled = true;
        }
        if released {
            self.clipboard_ui.paste_handled = false;
        }
        if mods.command && ui.input(|i| i.key_pressed(egui::Key::A)) {
            self.select_all_pixels();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, Raster};
    fn frame(ctx: &egui::Context, editor: &mut ImageEditor, events: Vec<egui::Event>) {
        let _ = ctx.run_ui(
            egui::RawInput {
                modifiers: egui::Modifiers::COMMAND,
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    editor.clipboard_shortcuts(ui);
                });
            },
        );
    }
    #[test]
    fn native_style_copy_and_key_release_paste_are_reversible_and_not_duplicated() {
        let ctx = egui::Context::default();
        let mut editor = ImageEditor::new(
            Document::from_raster(Raster::solid(4, 4, [1, 2, 3, 255]).unwrap()).unwrap(),
        );
        editor
            .document
            .select_rectangle([1.0, 1.0], [3.0, 3.0])
            .unwrap();
        let before = editor.document.clone();
        frame(&ctx, &mut editor, vec![egui::Event::Copy]);
        assert_eq!(editor.document, before);
        let released = egui::Event::Key {
            key: egui::Key::V,
            physical_key: Some(egui::Key::V),
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        };
        frame(&ctx, &mut editor, vec![released.clone()]);
        assert_eq!(editor.document.layers().len(), 2);
        editor.restore_history(false);
        assert_eq!(editor.document, before);
        frame(&ctx, &mut editor, vec![egui::Event::Paste(String::new())]);
        assert_eq!(editor.document.layers().len(), 2);
        frame(&ctx, &mut editor, vec![released]);
        assert_eq!(editor.document.layers().len(), 2);
    }
    #[test]
    fn clipboard_survives_document_replacement_and_current_paste_is_one_undo() {
        let mut editor = ImageEditor::new(
            Document::from_raster(Raster::solid(3, 3, [40, 80, 120, 255]).unwrap()).unwrap(),
        );
        editor.copy_pixels(false);
        editor.replace(Document::new(3, 3).unwrap());
        let before = editor.document.clone();
        editor.paste_clipboard(false);
        assert_eq!(editor.document.layers().len(), 1);
        assert_eq!(
            editor.document.flatten().pixel(1, 1),
            Some([40, 80, 120, 255])
        );
        editor.restore_history(false);
        assert_eq!(editor.document, before);
    }
    fn pasted_editor(new_layer: bool) -> (ImageEditor, Document) {
        let mut editor = ImageEditor::new(
            Document::from_raster(Raster::solid(8, 8, [0, 0, 200, 255]).unwrap()).unwrap(),
        );
        editor.clipboard_ui.pixels = Some(PixelClipboard::from_raster(
            Raster::solid(2, 2, [200, 0, 0, 128]).unwrap(),
        ));
        editor.clipboard_ui.at = [1, 1];
        editor.clipboard_ui.internal_only = true;
        let before = editor.document.clone();
        editor.paste_clipboard(new_layer);
        (editor, before)
    }
    #[test]
    fn moving_paste_restores_background_for_both_targets_and_undo_redo_keeps_final_position() {
        for new_layer in [false, true] {
            let (mut editor, before) = pasted_editor(new_layer);
            assert_eq!(editor.manual_ui.tool, super::super::ManualTool::Hand);
            editor.move_pasted_object([-1, 2]);
            editor.move_pasted_object([4, 3]);
            assert_eq!(
                editor.document.layers().len(),
                if new_layer { 2 } else { 1 }
            );
            assert_eq!(
                editor.document.flatten().pixel(1, 1),
                Some([0, 0, 200, 255])
            );
            assert_eq!(
                editor.document.flatten().pixel(4, 3),
                Some([100, 0, 100, 255])
            );
            let placed = editor.document.clone();
            editor.restore_history(false);
            assert_eq!(editor.document, before);
            assert!(!editor.has_pasted_object());
            editor.restore_history(true);
            assert_eq!(editor.document, placed);
        }
    }
    #[test]
    fn selected_mask_copy_and_paste_move_mask_only_and_cancel_is_one_undo() {
        let mut editor = ImageEditor::new(Document::new(8, 8).unwrap());
        let mut mask = vec![0; 64];
        mask[9] = 64;
        mask[10] = 255;
        editor.document.set_mask(mask).unwrap();
        let before = editor.document.clone();
        editor.set_edit_target(super::super::EditTarget::Mask);
        editor.copy_pixels(false);
        assert!(editor.clipboard_ui.pixels.as_ref().unwrap().is_mask());
        assert_eq!(editor.document, before);
        editor.paste_clipboard(true);
        editor.move_pasted_object([4, 3]);
        assert_eq!(editor.document.layers(), before.layers());
        assert_eq!(
            editor.document.mask()[9],
            64,
            "paste retains the original mask outside its destination"
        );
        assert_eq!(editor.document.mask()[28], 64);
        assert_eq!(editor.document.mask()[29], 255);
        assert!(editor.generation_ui.show_mask);
        let ctx = egui::Context::default();
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(editor.document, before);
    }
    #[test]
    fn changing_tool_or_document_finalizes_paste_without_overwriting_later_edits() {
        let (mut editor, _) = pasted_editor(false);
        editor.select_manual_tool(super::super::ManualTool::Brush);
        let placed = editor.document.clone();
        editor.move_pasted_object([5, 5]);
        assert_eq!(editor.document, placed);
        let (mut editor, _) = pasted_editor(false);
        editor.document.clear_mask();
        editor
            .document
            .add_layer(
                "another".into(),
                Raster::solid(8, 8, [1, 2, 3, 255]).unwrap(),
            )
            .unwrap();
        let changed = editor.document.clone();
        editor.move_pasted_object([5, 5]);
        assert_eq!(editor.document, changed);
        assert!(!editor.has_pasted_object());
    }
    fn canvas_frame(
        ctx: &egui::Context,
        editor: &mut ImageEditor,
        events: Vec<egui::Event>,
    ) -> egui::Rect {
        let mut rect = egui::Rect::NOTHING;
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::Vec2::splat(200.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    editor.canvas_pan(ui);
                    let (_, response) = ui.allocate_exact_size(
                        egui::Vec2::splat(64.0),
                        egui::Sense::click_and_drag(),
                    );
                    rect = response.rect;
                    editor.manual_canvas(ui, &response);
                });
            },
        );
        rect
    }
    #[test]
    fn hand_pointer_drag_moves_fragment_at_zoom_while_secondary_drag_pans_canvas() {
        let ctx = egui::Context::default();
        let (mut editor, _) = pasted_editor(false);
        editor.zoom = 8.0;
        let rect = canvas_frame(&ctx, &mut editor, vec![]);
        let from = rect.min + egui::vec2(12.0, 12.0);
        let to = from + egui::vec2(24.0, 16.0);
        let button = |pos, button, pressed| egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        canvas_frame(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(from),
                button(from, egui::PointerButton::Primary, true),
            ],
        );
        canvas_frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(to)]);
        canvas_frame(
            &ctx,
            &mut editor,
            vec![button(to, egui::PointerButton::Primary, false)],
        );
        assert_eq!(editor.clipboard_ui.floating.as_ref().unwrap().at, [4, 3]);
        assert_eq!(editor.navigation.offset, egui::Vec2::ZERO);
        assert_eq!(
            editor.document.flatten().pixel(1, 1),
            Some([0, 0, 200, 255])
        );
        let placed = editor.document.clone();
        canvas_frame(
            &ctx,
            &mut editor,
            vec![button(to, egui::PointerButton::Secondary, true)],
        );
        let end = to + egui::vec2(10.0, 6.0);
        canvas_frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(end)]);
        canvas_frame(
            &ctx,
            &mut editor,
            vec![button(end, egui::PointerButton::Secondary, false)],
        );
        assert_eq!(editor.document, placed);
        assert_eq!(editor.navigation.offset, egui::vec2(10.0, 6.0));
    }
    #[test]
    fn enter_finalizes_placement_and_next_hand_drag_pans_without_moving_pixels() {
        let ctx = egui::Context::default();
        let (mut editor, _) = pasted_editor(false);
        editor.move_pasted_object([4, 3]);
        let placed = editor.document.clone();
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(!editor.has_pasted_object());
        assert_eq!(editor.manual_ui.tool, super::super::ManualTool::Hand);
        let rect = canvas_frame(&ctx, &mut editor, vec![]);
        let from = rect.center();
        let to = from + egui::vec2(12.0, 8.0);
        canvas_frame(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(from),
                egui::Event::PointerButton {
                    pos: from,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        canvas_frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(to)]);
        canvas_frame(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerButton {
                pos: to,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(editor.document, placed);
        assert_eq!(editor.navigation.offset, egui::vec2(12.0, 8.0));
    }
    #[test]
    fn paste_target_comes_only_from_global_mode_including_cross_target_copy() {
        let mut editor = ImageEditor::new(Document::new(8, 8).unwrap());
        editor.clipboard_ui.pixels = Some(PixelClipboard::from_raster(
            Raster::solid(2, 2, [255, 255, 255, 128]).unwrap(),
        ));
        editor.clipboard_ui.internal_only = true;
        editor.set_edit_target(super::super::EditTarget::Mask);
        let layers = editor.document.layers().to_vec();
        editor.paste_clipboard(true);
        assert_eq!(editor.document.layers(), layers);
        assert_eq!(editor.document.mask()[0], 128);
        editor.copy_pixels(false);
        editor.set_edit_target(super::super::EditTarget::Raster);
        let mask = editor.document.mask().to_vec();
        editor.paste_clipboard(true);
        assert_eq!(editor.document.layers().len(), 2);
        assert_eq!(editor.document.mask(), mask);
        assert_eq!(
            editor.document.layers()[1].raster().pixel(0, 0),
            Some([128, 128, 128, 255])
        );
        assert_eq!(editor.manual_ui.tool, super::super::ManualTool::Hand);
    }
    #[test]
    fn cutting_manual_selection_keeps_ai_mask_and_is_one_undo() {
        let mut editor = ImageEditor::new(
            Document::from_raster(Raster::solid(8, 8, [90, 80, 70, 255]).unwrap()).unwrap(),
        );
        editor.document.set_mask(vec![77; 64]).unwrap();
        editor
            .document
            .select_area_rectangle([1.0, 1.0], [3.0, 3.0], crate::SelectionOp::Replace)
            .unwrap();
        let before = editor.document.clone();
        editor.cut_pixels();
        assert_eq!(
            editor
                .clipboard_ui
                .pixels
                .as_ref()
                .unwrap()
                .raster()
                .width(),
            2
        );
        assert_eq!(
            editor.document.layers()[0].raster().pixel(1, 1).unwrap()[3],
            0
        );
        assert_eq!(
            editor.document.layers()[0].raster().pixel(0, 0).unwrap()[3],
            255
        );
        assert_eq!(editor.document.mask(), before.mask());
        editor.restore_history(false);
        assert_eq!(editor.document, before);
    }
    #[test]
    fn selection_follows_pasted_object_so_copy_after_drag_uses_its_new_position() {
        let (mut editor, _) = pasted_editor(true);
        editor.move_pasted_object([4, 3]);
        assert_eq!(editor.document.selection()[9], 0);
        assert_eq!(editor.document.selection()[28], 255);
        assert!(editor.copy_pixels(false));
        let copied = editor.clipboard_ui.pixels.as_ref().unwrap();
        assert_eq!(copied.origin(), [4, 3]);
        assert_eq!(copied.raster().pixel(0, 0), Some([200, 0, 0, 128]));
    }
    #[test]
    fn layer_mask_copy_paste_placement_and_cut_leave_raster_and_ai_intact() {
        let mut doc =
            Document::from_raster(Raster::solid(8, 8, [20, 40, 60, 255]).unwrap()).unwrap();
        doc.set_mask(vec![77; 64]).unwrap();
        let mut mask = vec![0; 64];
        mask[9] = 128;
        mask[10] = 255;
        doc.set_layer_mask(mask).unwrap();
        doc.select_area_rectangle([1.0, 1.0], [3.0, 2.0], crate::SelectionOp::Replace)
            .unwrap();
        let mut e = ImageEditor::new(doc);
        e.set_mask_target(crate::MaskTarget::Layer(0)).unwrap();
        let before = e.document.clone();
        assert!(e.copy_pixels(false));
        e.clipboard_ui.internal_only = true;
        assert_eq!(
            e.clipboard_ui.pixels.as_ref().unwrap().raster().pixel(0, 0),
            Some([128, 128, 128, 255])
        );
        e.paste_clipboard(true);
        e.move_pasted_object([4, 3]);
        assert_eq!(e.document.layers().len(), 1);
        assert_eq!(e.document.layers()[0].raster(), before.layers()[0].raster());
        assert_eq!(e.document.mask(), before.mask());
        assert_eq!(e.document.layers()[0].mask().unwrap().pixels()[28], 128);
        assert_eq!(e.document.layers()[0].mask().unwrap().pixels()[29], 255);
        assert_eq!(e.manual_ui.tool, super::super::ManualTool::Hand);
        e.restore_history(false);
        assert_eq!(e.document, before);
        e.set_mask_target(crate::MaskTarget::Layer(0)).unwrap();
        e.cut_pixels();
        assert_eq!(e.document.layers()[0].mask().unwrap().pixels()[9], 0);
        assert_eq!(e.document.mask(), before.mask());
        e.restore_history(false);
        assert_eq!(e.document, before);
    }
}
