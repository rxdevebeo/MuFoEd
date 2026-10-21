//! Layer-mask controls keep one Raster/Mask mode and an explicit mask destination.
use super::{EditTarget, ImageEditor};
use crate::{EditError, LayerBlendMode, MaskTarget};
#[derive(Clone, Copy)]
pub(super) enum MaskAction {
    Reveal,
    Hide,
    Selection,
    Inpaint,
    Toggle,
    Invert,
    Apply,
    Remove,
}
impl ImageEditor {
    /// Current destination within the single global Mask mode.
    pub fn mask_target(&self) -> MaskTarget {
        self.manual_ui.mask_target
    }
    /// Select an existing layer mask (also selects its layer), or the shared AI mask.
    pub fn set_mask_target(&mut self, target: MaskTarget) -> Result<(), EditError> {
        self.document.target_mask(target)?;
        self.reset_manual_mask_context();
        if let MaskTarget::Layer(i) = target {
            self.document.select(i)?;
        }
        self.manual_ui.mask_target = target;
        self.set_edit_target(EditTarget::Mask);
        match target {
            MaskTarget::Inpaint => self.generation_ui.show_mask = true,
            MaskTarget::Layer(_) => self.manual_ui.show_layer_mask = true,
        }
        self.changed = true;
        Ok(())
    }
    // Explicit AI controls must not silently keep displaying another mask.
    pub(super) fn show_inpaint_mask(&mut self, show: bool) {
        if matches!(self.mask_target(), MaskTarget::Layer(_)) {
            self.reset_manual_mask_context();
            self.manual_ui.mask_target = MaskTarget::Inpaint;
        }
        self.generation_ui.show_mask = show;
        self.changed = true;
    }
    pub(super) fn mask_source_raster(&self) -> crate::Raster {
        self.document
            .target_mask_raster(self.mask_target())
            .expect("validated UI mask target")
    }
    pub(super) fn display_mask(&self) -> Option<&[u8]> {
        if self.edit_target() == EditTarget::Mask
            && matches!(self.mask_target(), MaskTarget::Layer(_))
        {
            if self.manual_ui.show_layer_mask {
                self.document.target_mask(self.mask_target()).ok()
            } else {
                None
            }
        } else if self.generation_ui.show_mask {
            Some(self.document.mask())
        } else {
            None
        }
    }
    pub(super) fn mask_destination_control(&mut self, ui: &mut egui::Ui) {
        if self.edit_target() != EditTarget::Mask {
            return;
        }
        ui.menu_button(
            if matches!(self.mask_target(), MaskTarget::Layer(_)) {
                "Маска слоя"
            } else {
                "Маска ИИ"
            },
            |ui| {
                if ui
                    .selectable_label(
                        self.mask_target() == MaskTarget::Inpaint,
                        "Маска ИИ / перерисовка",
                    )
                    .clicked()
                {
                    if let Err(e) = self.set_mask_target(MaskTarget::Inpaint) {
                        self.status = e.to_string();
                    }
                    ui.close();
                }
                let i = self.document.active();
                if ui
                    .add_enabled(
                        self.document.layers()[i].mask().is_some(),
                        egui::Button::new("Маска текущего слоя"),
                    )
                    .clicked()
                {
                    if let Err(e) = self.set_mask_target(MaskTarget::Layer(i)) {
                        self.status = e.to_string();
                    }
                    ui.close();
                }
                let changed = if matches!(self.mask_target(), MaskTarget::Layer(_)) {
                    ui.checkbox(&mut self.manual_ui.show_layer_mask, "Показывать покрытие")
                        .changed()
                } else {
                    ui.checkbox(&mut self.generation_ui.show_mask, "Показывать покрытие")
                        .changed()
                };
                self.changed |= changed;
            },
        );
    }
    pub(super) fn layer_mask_action(&mut self, index: usize, action: MaskAction) {
        self.reset_manual_mask_context();
        let old = self.document.clone();
        self.edit(|d| {
            d.select(index)?;
            match action {
                MaskAction::Reveal => d.add_layer_mask(true),
                MaskAction::Hide => d.add_layer_mask(false),
                MaskAction::Selection => {
                    if d.selection().is_empty() {
                        return Err(EditError::Invalid("Сначала выделите область".into()));
                    }
                    d.set_layer_mask(d.selection().to_vec())
                }
                MaskAction::Inpaint => d.set_layer_mask(d.mask().to_vec()),
                MaskAction::Toggle => {
                    let enabled = d.layers()[index]
                        .mask()
                        .is_some_and(crate::LayerMask::enabled);
                    d.set_layer_mask_enabled(!enabled)
                }
                MaskAction::Invert => d.invert_layer_mask(),
                MaskAction::Apply => d.apply_layer_mask(),
                MaskAction::Remove => d.remove_layer_mask(),
            }
        });
        if self.document == old {
            return;
        }
        if self.document.layers()[index].mask().is_some() {
            if let Err(e) = self.set_mask_target(MaskTarget::Layer(index)) {
                self.status = e.to_string();
            }
            if matches!(action, MaskAction::Reveal) {
                self.color = egui::Color32::BLACK;
            }
            if matches!(action, MaskAction::Hide) {
                self.color = egui::Color32::WHITE;
            }
        } else {
            self.manual_ui.mask_target = MaskTarget::Inpaint;
            self.set_edit_target(EditTarget::Raster);
            self.changed = true;
        }
    }
    pub(super) fn mask_menu(&self, ui: &mut egui::Ui, index: usize) -> Option<MaskAction> {
        let layer = &self.document.layers()[index];
        let mut action = None;
        ui.add_enabled_ui(!layer.locked(), |ui| {
            let options: &[(MaskAction, &str)] = if let Some(mask) = layer.mask() {
                &[
                    (
                        MaskAction::Toggle,
                        if mask.enabled() {
                            "Отключить"
                        } else {
                            "Включить"
                        },
                    ),
                    (MaskAction::Invert, "Инвертировать"),
                    (MaskAction::Apply, "Применить в альфа-канал"),
                    (MaskAction::Remove, "Удалить маску"),
                ]
            } else {
                &[
                    (MaskAction::Reveal, "Белая — показать всё"),
                    (MaskAction::Hide, "Чёрная — скрыть всё"),
                    (MaskAction::Selection, "Из ручного выделения"),
                    (MaskAction::Inpaint, "Из маски ИИ"),
                ]
            };
            for &(a, label) in options {
                let enabled =
                    !matches!(a, MaskAction::Selection) || !self.document.selection().is_empty();
                if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                    action = Some(a);
                    ui.close();
                }
            }
        });
        action
    }
}
pub(super) fn blend_label(mode: LayerBlendMode) -> &'static str {
    match mode {
        LayerBlendMode::Normal => "Обычный",
        LayerBlendMode::Multiply => "Multiply",
        LayerBlendMode::Screen => "Screen",
        LayerBlendMode::Overlay => "Overlay",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, Raster, Tool};
    #[test]
    fn mask_commands_common_tools_and_history_preserve_independent_planes() {
        let mut doc =
            Document::from_raster(Raster::solid(8, 8, [80, 120, 160, 255]).unwrap()).unwrap();
        doc.set_mask(vec![77; 64]).unwrap();
        let mut e = ImageEditor::new(doc);
        let before = e.document.clone();
        e.layer_mask_action(0, MaskAction::Reveal);
        assert_eq!(e.edit_target(), EditTarget::Mask);
        assert_eq!(e.mask_target(), MaskTarget::Layer(0));
        assert_eq!(e.document.mask(), before.mask());
        let created = e.document.clone();
        e.edit_manual(|d| d.stroke([2.0, 2.0], [4.0, 2.0], 1.0, [0, 0, 0, 255], Tool::Brush));
        assert_ne!(e.document.layers()[0].mask(), created.layers()[0].mask());
        assert_eq!(e.document.layers()[0].raster(), before.layers()[0].raster());
        assert_eq!(e.document.mask(), before.mask());
        e.restore_history(false);
        assert_eq!(e.document, created);
        assert_eq!(e.mask_target(), MaskTarget::Inpaint);
        assert_eq!(e.edit_target(), EditTarget::Raster);
        e.set_mask_target(MaskTarget::Layer(0)).unwrap();
        e.layer_mask_action(0, MaskAction::Toggle);
        assert!(!e.document.layers()[0].mask().unwrap().enabled());
        e.layer_mask_action(0, MaskAction::Invert);
        assert_eq!(e.document.layers()[0].mask().unwrap().pixels(), [0; 64]);
        e.document.set_layer_locked(true);
        let locked = e.document.clone();
        e.layer_mask_action(0, MaskAction::Remove);
        assert_eq!(e.document, locked);
        e.document.set_layer_locked(false);
        e.layer_mask_action(0, MaskAction::Remove);
        assert!(e.document.layers()[0].mask().is_none());
        assert_eq!(e.edit_target(), EditTarget::Raster);
    }
    #[test]
    fn clearing_or_reducing_the_stack_cannot_leave_a_stale_mask_destination() {
        for clear in [false, true] {
            let mut doc = Document::new(8, 8).unwrap();
            doc.add_layer("Upper".into(), Raster::solid(8, 8, [255; 4]).unwrap())
                .unwrap();
            doc.add_layer_mask(true).unwrap();
            let mut e = ImageEditor::new(doc);
            e.set_mask_target(MaskTarget::Layer(1)).unwrap();
            if clear {
                e.edit(Document::clear_canvas);
            } else {
                e.edit(Document::retain_active_layer);
            }
            assert_eq!(e.mask_target(), MaskTarget::Inpaint);
            assert_eq!(e.edit_target(), EditTarget::Raster);
            e.set_edit_target(EditTarget::Mask);
            assert_eq!(e.mask_source_raster().width(), 8);
        }
    }
    #[test]
    fn explicit_ai_overlay_control_selects_the_ai_plane_and_keeps_requested_visibility() {
        let mut doc = Document::new(8, 8).unwrap();
        doc.add_layer_mask(true).unwrap();
        let mut e = ImageEditor::new(doc);
        e.set_mask_target(MaskTarget::Layer(0)).unwrap();
        let before = e.document.clone();
        e.show_inpaint_mask(false);
        assert_eq!(e.mask_target(), MaskTarget::Inpaint);
        assert_eq!(e.edit_target(), EditTarget::Mask);
        assert!(e.display_mask().is_none());
        e.show_inpaint_mask(true);
        assert_eq!(e.display_mask(), Some(e.document.mask()));
        assert_eq!(e.document, before);
    }
}
