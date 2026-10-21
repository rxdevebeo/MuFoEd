//! The ribbon: tabs of grouped commands.

use super::{Command, Gallery, RibbonTab, THUMB_RANGE};
use crate::batch::Recipe;
use crate::catalog::SortKey;
use crate::Locale;

impl RibbonTab {
    const ALL: [Self; 4] = [Self::Home, Self::Image, Self::Batch, Self::Files];

    fn title(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Image => "Picture",
            Self::Batch => "Batch",
            Self::Files => "Files",
        }
    }
}

pub(super) fn sort_name(key: SortKey) -> &'static str {
    match key {
        SortKey::Name => "Name",
        SortKey::Modified => "Date modified",
        SortKey::Size => "Size",
        SortKey::Type => "Type",
    }
}

/// A titled group of controls, followed by a divider.
fn group(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        ui.horizontal(add);
        ui.small(title);
    });
    ui.separator();
}

impl Gallery {
    pub(super) fn ribbon_ui(&mut self, ui: &mut egui::Ui) {
        let locale = self.locale.clone();
        ui.horizontal(|ui| {
            for tab in RibbonTab::ALL {
                ui.selectable_value(&mut self.tab, tab, locale.text(tab.title()));
            }
        });
        ui.separator();
        let tab = self.tab;
        ui.horizontal(|ui| match tab {
            RibbonTab::Home => self.home_tab(ui, &locale),
            RibbonTab::Image => self.picture_tab(ui, &locale),
            RibbonTab::Batch => self.batch_tab(ui, &locale),
            RibbonTab::Files => self.files_tab(ui, &locale),
        });
    }

    /// A button for `command`, enabled when it can run.
    fn command(&mut self, ui: &mut egui::Ui, command: Command, label: &str) {
        let button = egui::Button::new(label);
        if ui.add_enabled(self.can_execute(command), button).clicked() {
            self.execute(command);
        }
    }

    fn home_tab(&mut self, ui: &mut egui::Ui, locale: &Locale) {
        group(ui, locale.text("Folder"), |ui| {
            self.command(ui, Command::OpenFolder, locale.text("Open folder…"));
            self.command(ui, Command::Refresh, locale.text("Refresh"));
            if ui
                .checkbox(&mut self.settings.recursive, locale.text("Sub-folders"))
                .changed()
            {
                self.execute(Command::Refresh);
            }
        });
        group(ui, locale.text("Selection"), |ui| {
            self.command(ui, Command::SelectAll, locale.text("All"));
            self.command(ui, Command::SelectNone, locale.text("None"));
            self.command(ui, Command::InvertSelection, locale.text("Invert"));
        });
        group(ui, locale.text("View"), |ui| {
            ui.add(
                egui::Slider::new(&mut self.settings.thumb_size, THUMB_RANGE)
                    .show_value(false)
                    .text(locale.text("Size")),
            );
            let mut changed = false;
            egui::ComboBox::from_id_salt("image-gallery-sort")
                .selected_text(locale.text(sort_name(self.settings.sort)))
                .show_ui(ui, |ui| {
                    for key in SortKey::ALL {
                        changed |= ui
                            .selectable_value(
                                &mut self.settings.sort,
                                key,
                                locale.text(sort_name(key)),
                            )
                            .changed();
                    }
                });
            let arrow = if self.settings.descending {
                "↓"
            } else {
                "↑"
            };
            if ui
                .selectable_label(self.settings.descending, arrow)
                .on_hover_text(locale.text("Reverse order"))
                .clicked()
            {
                self.settings.descending = !self.settings.descending;
                changed = true;
            }
            if changed {
                self.resort();
            }
            ui.checkbox(&mut self.settings.show_names, locale.text("Names"));
        });
        group(ui, locale.text("Open"), |ui| {
            self.command(ui, Command::OpenViewer, locale.text("View"));
            self.command(ui, Command::OpenInEditor, locale.text("Open in editor"));
        });
    }

    fn picture_tab(&mut self, ui: &mut egui::Ui, locale: &Locale) {
        group(ui, locale.text("Turn"), |ui| {
            self.command(ui, Command::RotateLeft, locale.text("Rotate left"));
            self.command(ui, Command::RotateRight, locale.text("Rotate right"));
            self.command(ui, Command::Rotate180, locale.text("Rotate 180°"));
        });
        group(ui, locale.text("Mirror"), |ui| {
            self.command(
                ui,
                Command::FlipHorizontal,
                locale.text("Flip horizontally"),
            );
            self.command(ui, Command::FlipVertical, locale.text("Flip vertically"));
        });
        group(ui, locale.text("Metadata"), |ui| {
            self.command(ui, Command::StripMetadata, locale.text("Remove metadata"));
        });
        self.background_group(ui, locale);
        group(ui, locale.text("Options"), |ui| {
            ui.checkbox(&mut self.settings.lossless_jpeg, locale.text("JPEG without loss"))
                .on_hover_text(locale.text(
                    "Turns and mirrors of JPEG files are recorded in the EXIF orientation; the compressed picture is untouched",
                ));
        });
        ui.weak(locale.text("These commands change the selected files in place."));
    }

    fn background_group(&mut self, ui: &mut egui::Ui, locale: &Locale) {
        group(ui, locale.text("Background"), |ui| {
            let found = self.background_model_found();
            let reason = if found {
                self.settings.background.model_path().display().to_string()
            } else {
                locale
                    .text("Choose or download the BiRefNet model first")
                    .to_owned()
            };
            let button = egui::Button::new(locale.text("Remove background"));
            if ui
                .add_enabled(self.can_execute(Command::RemoveBackground), button)
                .on_hover_text(
                    locale.text(
                        "Writes a PNG copy with a transparent background next to each picture",
                    ),
                )
                .on_disabled_hover_text(reason.as_str())
                .clicked()
            {
                self.execute(Command::RemoveBackground);
            }
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if ui.small_button(locale.text("Model…")).on_hover_text(reason.as_str()).clicked() {
                        self.events.push(super::GalleryEvent::PickModel);
                    }
                    ui.checkbox(&mut self.settings.background.gpu, locale.text("GPU"))
                        .on_hover_text(locale.text("CUDA through onnxruntime, when it is installed"));
                });
                if !found {
                    ui.horizontal(|ui| {
                        let variant = &mut self.settings.background.variant;
                        egui::ComboBox::from_id_salt("image-gallery-birefnet")
                            .selected_text(variant.label())
                            .show_ui(ui, |ui| {
                                for option in crate::background::Variant::ALL {
                                    ui.selectable_value(variant, option, option.label());
                                }
                            });
                        if ui
                            .add_enabled(self.can_execute(Command::DownloadModel), egui::Button::new(locale.text("Download")).small())
                            .on_hover_text(locale.text("The official BiRefNet release, checked and recorded with its SHA-256"))
                            .clicked()
                        {
                            self.execute(Command::DownloadModel);
                        }
                    });
                }
            });
        });
    }

    fn batch_tab(&mut self, ui: &mut egui::Ui, locale: &Locale) {
        group(ui, locale.text("Recipe"), |ui| {
            let current = self.settings.recipe.name.clone();
            let mut chosen = None;
            egui::ComboBox::from_id_salt("image-gallery-preset")
                .width(220.0)
                .selected_text(locale.text(&current))
                .show_ui(ui, |ui| {
                    for preset in Recipe::presets()
                        .into_iter()
                        .chain(self.settings.presets.iter().cloned())
                    {
                        if ui
                            .selectable_label(preset.name == current, locale.text(&preset.name))
                            .clicked()
                        {
                            chosen = Some(preset);
                        }
                    }
                });
            if let Some(preset) = chosen {
                self.settings.recipe = preset;
            }
            if ui.button(locale.text("Save as preset")).clicked() {
                let recipe = self.settings.recipe.clone();
                self.settings
                    .presets
                    .retain(|preset| preset.name != recipe.name);
                self.settings.presets.push(recipe);
            }
            let user = self
                .settings
                .presets
                .iter()
                .any(|preset| preset.name == current);
            if ui
                .add_enabled(user, egui::Button::new(locale.text("Delete preset")))
                .clicked()
            {
                self.settings
                    .presets
                    .retain(|preset| preset.name != current);
            }
        });
        group(ui, locale.text("Run"), |ui| {
            let count = self.targets().len();
            let label = locale.format("Process {} files", &[count.to_string()]);
            self.command(ui, Command::RunRecipe, &label);
        });
        ui.weak(locale.text("The recipe is edited in the side panel."));
    }

    fn files_tab(&mut self, ui: &mut egui::Ui, locale: &Locale) {
        group(ui, locale.text("Copy and move"), |ui| {
            self.command(ui, Command::CopyTo, locale.text("Copy to…"));
            self.command(ui, Command::MoveTo, locale.text("Move to…"));
        });
        group(ui, locale.text("Rename"), |ui| {
            let edit =
                egui::TextEdit::singleline(&mut self.settings.rename_template).desired_width(200.0);
            let mut changed = ui.add(edit).changed();
            changed |= ui
                .add(egui::DragValue::new(&mut self.settings.rename_start).range(0..=1_000_000))
                .on_hover_text(locale.text("First number"))
                .changed();
            if changed {
                self.rename_plan = None;
            }
            self.command(ui, Command::Rename, locale.text("Rename"));
        });
        ui.weak("{name} {n:3} {date} {time} {w} {h} {camera} {folder} {ext}");
    }
}
