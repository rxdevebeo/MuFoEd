//! The side panel: properties, the recipe editor, the rename preview and the
//! last job's report.

// Byte counts shown with one decimal.
#![allow(clippy::cast_precision_loss)]

use egui::RichText;

use super::{Gallery, GalleryEvent, Purpose, RibbonTab};
use crate::batch::{Conflict, Destination, SkipReason};
use crate::encode::{OutputFormat, PngLevel};
use crate::exif::Stamp;
use crate::ops::{
    Align, Anchor, Axis, CanvasMode, CropMode, Filter, Operation, QuarterTurn, ResizeMode, Tools,
};
use crate::{Format, Locale};

/// Bytes as B, KB, MB or GB.
fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn row(ui: &mut egui::Ui, label: &str, value: impl Into<egui::WidgetText>) {
    ui.weak(label);
    ui.label(value);
    ui.end_row();
}

impl Gallery {
    pub(super) fn side_ui(&mut self, ui: &mut egui::Ui) {
        let locale = self.locale.clone();
        egui::ScrollArea::vertical()
            .id_salt("image-gallery-side-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.report_ui(ui, &locale);
                let tab = self.tab;
                match tab {
                    RibbonTab::Batch => self.recipe_ui(ui, &locale),
                    RibbonTab::Files => self.rename_ui(ui, &locale),
                    RibbonTab::Home | RibbonTab::Image => self.properties_ui(ui, &locale),
                }
            });
    }

    fn report_ui(&mut self, ui: &mut egui::Ui, locale: &Locale) {
        let Some(report) = &self.report else {
            return;
        };
        let mut close = false;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(locale.text(&report.title));
                if ui.small_button("✕").clicked() {
                    close = true;
                }
            });
            ui.label(locale.format("Written: {}", &[report.written.to_string()]));
            if report.lossless > 0 {
                ui.weak(locale.format("Without re-encoding: {}", &[report.lossless.to_string()]));
            }
            if !report.skipped.is_empty() {
                let count = |wanted| {
                    report
                        .skipped
                        .iter()
                        .filter(|(_, reason)| *reason == wanted)
                        .count()
                };
                for (reason, key) in [
                    (SkipReason::Exists, "Skipped, the name is taken: {}"),
                    (SkipReason::NothingToDo, "Skipped, nothing to change: {}"),
                    (SkipReason::Cancelled, "Cancelled: {}"),
                ] {
                    let n = count(reason);
                    if n > 0 {
                        ui.label(locale.format(key, &[n.to_string()]));
                    }
                }
            }
            if !report.failed.is_empty() {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    locale.format("Failed: {}", &[report.failed.len().to_string()]),
                );
                for (path, error) in report.failed.iter().take(50) {
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    ui.small(format!("{name}: {error}"));
                }
            }
        });
        if close {
            self.report = None;
        }
        ui.add_space(6.0);
    }

    // One form, top to bottom.
    #[allow(clippy::too_many_lines)]
    fn properties_ui(&mut self, ui: &mut egui::Ui, locale: &Locale) {
        ui.heading(locale.text("Properties"));
        let selected = self.selection();
        if selected.len() > 1 {
            let total: u64 = self
                .view
                .iter()
                .map(|&index| &self.entries[index])
                .filter(|entry| self.selection.contains(&entry.path))
                .map(|entry| entry.size)
                .sum();
            ui.label(locale.format("{} selected", &[selected.len().to_string()]));
            ui.label(human_size(total));
            return;
        }
        let path = selected.into_iter().next().or_else(|| self.focus_path());
        let Some(entry) = path
            .and_then(|path| self.position_of(&path))
            .and_then(|at| self.entry_at(at))
        else {
            ui.weak(locale.text("Select a picture to see its properties."));
            return;
        };
        let info = self.info_of(&entry.path);
        egui::Grid::new("image-gallery-properties")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                row(ui, locale.text("Name"), RichText::new(&entry.name).strong());
                if let Some(folder) = entry.path.parent() {
                    row(ui, locale.text("Folder"), folder.display().to_string());
                }
                row(ui, locale.text("File size"), human_size(entry.size));
                if let Some(stamp) = entry.modified.and_then(Stamp::from_system_time) {
                    row(ui, locale.text("Modified (UTC)"), stamp.to_string());
                }
                let format = info.map_or(entry.format, |info| info.format);
                row(ui, locale.text("Format"), format.name());
                if let Some(info) = info {
                    row(
                        ui,
                        locale.text("Dimensions"),
                        format!("{} × {} px", info.width, info.height),
                    );
                    let megapixels = f64::from(info.width) * f64::from(info.height) / 1e6;
                    row(ui, locale.text("Megapixels"), format!("{megapixels:.1}"));
                    row(
                        ui,
                        locale.text("Colour"),
                        format!("{}, {} bit", locale.text(info.colour), info.bits),
                    );
                    let yes_no = |value| {
                        if value {
                            locale.text("yes")
                        } else {
                            locale.text("no")
                        }
                    };
                    row(ui, locale.text("ICC profile"), yes_no(info.has_icc));
                    row(ui, locale.text("EXIF"), yes_no(info.has_exif));
                    if let Some(summary) = &info.summary {
                        let fields = [
                            ("Camera", summary.camera()),
                            ("Lens", summary.lens.clone()),
                            ("Taken", summary.taken.map(|stamp| stamp.to_string())),
                            ("Exposure", summary.exposure.clone()),
                            ("Aperture", summary.aperture.clone()),
                            ("ISO", summary.iso.map(|iso| iso.to_string())),
                            ("Focal length", summary.focal_length.clone()),
                            ("Software", summary.software.clone()),
                        ];
                        for (label, value) in fields {
                            if let Some(value) = value {
                                row(ui, locale.text(label), value);
                            }
                        }
                        if summary.orientation != 1 {
                            row(
                                ui,
                                locale.text("Orientation"),
                                summary.orientation.to_string(),
                            );
                        }
                        if summary.has_gps {
                            row(ui, locale.text("Location"), locale.text("recorded (GPS)"));
                        }
                    }
                } else if !format.can_read() {
                    row(
                        ui,
                        "",
                        RichText::new(locale.text("This format cannot be read")).weak(),
                    );
                }
            });
    }

    fn rename_ui(&mut self, ui: &mut egui::Ui, locale: &Locale) {
        ui.heading(locale.text("Rename"));
        ui.weak(locale.text(
            "New names for the selection, by the template in the ribbon. The extension is kept.",
        ));
        let automatic = self.rename_plan.is_none() && self.targets().len() <= 200;
        if ui.button(locale.text("Preview")).clicked() || automatic {
            self.rename_plan = Some(self.plan_rename());
        }
        let Some(plan) = &self.rename_plan else {
            return;
        };
        egui::Grid::new("image-gallery-rename")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                for (from, to) in plan.iter().take(500) {
                    let name = |path: &std::path::Path| {
                        path.file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    };
                    ui.label(name(from));
                    let target = name(to);
                    if from == to {
                        ui.weak(target);
                    } else {
                        ui.strong(target);
                    }
                    ui.end_row();
                }
            });
        if plan.len() > 500 {
            ui.weak(locale.format("… and {} more", &[(plan.len() - 500).to_string()]));
        }
    }

    fn recipe_ui(&mut self, ui: &mut egui::Ui, locale: &Locale) {
        ui.heading(locale.text("Recipe"));
        ui.horizontal(|ui| {
            ui.label(locale.text("Name"));
            ui.text_edit_singleline(&mut self.settings.recipe.name);
        });
        ui.add_space(4.0);
        ui.strong(locale.text("Operations"));
        let tools = self.tools.clone();
        let operations = &mut self.settings.recipe.operations;
        let mut remove = None;
        let mut swap = None;
        let count = operations.len();
        for (index, operation) in operations.iter_mut().enumerate() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong(format!(
                        "{}. {}",
                        index + 1,
                        label(operation, locale, &tools)
                    ));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button("✕")
                            .on_hover_text(locale.text("Remove"))
                            .clicked()
                        {
                            remove = Some(index);
                        }
                        if ui
                            .add_enabled(index + 1 < count, egui::Button::new("↓").small())
                            .clicked()
                        {
                            swap = Some(index);
                        }
                        if ui
                            .add_enabled(index > 0, egui::Button::new("↑").small())
                            .clicked()
                        {
                            swap = Some(index - 1);
                        }
                    });
                });
                edit(ui, operation, locale);
            });
        }
        if let Some(index) = remove {
            operations.remove(index);
        }
        if let Some(index) = swap {
            operations.swap(index, index + 1);
        }
        if operations.is_empty() {
            ui.weak(locale.text("No operations: the files are only converted or copied."));
        }
        ui.menu_button(locale.text("Add operation"), |ui| {
            for operation in Operation::catalogue() {
                if ui.button(label(&operation, locale, &tools)).clicked() {
                    operations.push(operation);
                    ui.close();
                }
            }
            if tools.list().next().is_some() {
                ui.separator();
                for (id, name) in tools.list() {
                    if ui.button(name).clicked() {
                        operations.push(Operation::Tool { id: id.to_owned() });
                        ui.close();
                    }
                }
            }
        });
        ui.add_space(8.0);
        self.output_ui(ui, locale);
    }

    // One form, top to bottom.
    #[allow(clippy::too_many_lines)]
    fn output_ui(&mut self, ui: &mut egui::Ui, locale: &Locale) {
        ui.strong(locale.text("Output"));
        let output = &mut self.settings.recipe.output;
        let mut pick = false;
        egui::Grid::new("image-gallery-output")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label(locale.text("Format"));
                let shown = match output.format {
                    OutputFormat::Keep => locale.text("Keep the format").to_owned(),
                    OutputFormat::To(format) => format.name().to_owned(),
                };
                egui::ComboBox::from_id_salt("image-gallery-format")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut output.format,
                            OutputFormat::Keep,
                            locale.text("Keep the format"),
                        );
                        for format in Format::writable() {
                            ui.selectable_value(
                                &mut output.format,
                                OutputFormat::To(format),
                                format.name(),
                            );
                        }
                    });
                ui.end_row();

                let target = match output.format {
                    OutputFormat::To(format) => Some(format),
                    OutputFormat::Keep => None,
                };
                if target.is_none_or(|format| format == Format::Jpeg) {
                    ui.label(locale.text("JPEG quality"));
                    ui.add(egui::Slider::new(&mut output.encode.jpeg_quality, 1..=100));
                    ui.end_row();
                }
                if target.is_none_or(|format| format == Format::Png) {
                    ui.label(locale.text("PNG compression"));
                    ui.horizontal(|ui| {
                        let level = &mut output.encode.png_level;
                        ui.selectable_value(level, PngLevel::Fast, locale.text("fast"));
                        ui.selectable_value(level, PngLevel::Default, locale.text("normal"));
                        ui.selectable_value(level, PngLevel::Best, locale.text("best"));
                    });
                    ui.end_row();
                }
                if target == Some(Format::Avif) {
                    ui.label(locale.text("AVIF quality"));
                    ui.add(egui::Slider::new(&mut output.encode.avif_quality, 1..=100));
                    ui.end_row();
                    ui.label(locale.text("AVIF speed"));
                    ui.add(egui::Slider::new(&mut output.encode.avif_speed, 1..=10));
                    ui.end_row();
                }
                if target.is_some_and(|format| !format.has_alpha()) || target.is_none() {
                    ui.label(locale.text("Background"));
                    ui.color_edit_button_srgb(&mut output.encode.background)
                        .on_hover_text(locale.text("Fills transparency for formats without it"));
                    ui.end_row();
                }

                ui.label(locale.text("Save to"));
                ui.vertical(|ui| {
                    let same = matches!(output.destination, Destination::SameFolder);
                    if ui.radio(same, locale.text("the same folder")).clicked() {
                        output.destination = Destination::SameFolder;
                    }
                    let sub = matches!(output.destination, Destination::Subfolder(_));
                    ui.horizontal(|ui| {
                        if ui.radio(sub, locale.text("a sub-folder")).clicked() && !sub {
                            output.destination = Destination::Subfolder("processed".into());
                        }
                        if let Destination::Subfolder(name) = &mut output.destination {
                            ui.add(egui::TextEdit::singleline(name).desired_width(110.0));
                        }
                    });
                    let folder = match &output.destination {
                        Destination::Folder(folder) => Some(folder.display().to_string()),
                        _ => None,
                    };
                    ui.horizontal(|ui| {
                        if ui
                            .radio(folder.is_some(), locale.text("a folder…"))
                            .clicked()
                        {
                            pick = true;
                        }
                        if let Some(folder) = folder {
                            ui.small(folder);
                        }
                    });
                });
                ui.end_row();

                ui.label(locale.text("Name"));
                ui.add(egui::TextEdit::singleline(&mut output.template).desired_width(160.0))
                    .on_hover_text("{name} {n:3} {date} {time} {w} {h} {camera} {folder} {ext}");
                ui.end_row();
                ui.label(locale.text("First number"));
                ui.add(egui::DragValue::new(&mut output.start_index).range(0..=1_000_000));
                ui.end_row();

                ui.label(locale.text("If the name is taken"));
                egui::ComboBox::from_id_salt("image-gallery-conflict")
                    .selected_text(locale.text(conflict_name(output.conflict)))
                    .show_ui(ui, |ui| {
                        for conflict in [Conflict::Unique, Conflict::Skip, Conflict::Overwrite] {
                            ui.selectable_value(
                                &mut output.conflict,
                                conflict,
                                locale.text(conflict_name(conflict)),
                            );
                        }
                    });
                ui.end_row();

                ui.label(locale.text("SVG scale"));
                ui.add(
                    egui::DragValue::new(&mut output.vector_scale)
                        .range(0.1..=32.0)
                        .speed(0.05)
                        .suffix("×"),
                );
                ui.end_row();
            });
        ui.checkbox(&mut output.encode.keep_exif, locale.text("Keep EXIF"));
        ui.checkbox(
            &mut output.encode.keep_icc,
            locale.text("Keep the ICC profile"),
        );
        ui.checkbox(&mut output.auto_orient, locale.text("Turn upright by EXIF"));
        ui.checkbox(
            &mut output.lossless_jpeg,
            locale.text("JPEG turns without loss"),
        );
        ui.checkbox(
            &mut output.keep_modified_time,
            locale.text("Keep the modification time"),
        );
        if pick {
            self.events
                .push(GalleryEvent::PickDestination(Purpose::Output));
        }
    }
}

fn conflict_name(conflict: Conflict) -> &'static str {
    match conflict {
        Conflict::Unique => "add a number",
        Conflict::Skip => "skip the file",
        Conflict::Overwrite => "replace",
    }
}

fn turn_name(turn: QuarterTurn) -> &'static str {
    match turn {
        QuarterTurn::Right => "90° right",
        QuarterTurn::Left => "90° left",
        QuarterTurn::Half => "180°",
    }
}

fn filter_name(filter: Filter) -> &'static str {
    match filter {
        Filter::Nearest => "Nearest",
        Filter::Bilinear => "Bilinear",
        Filter::Bicubic => "Bicubic",
        Filter::Gaussian => "Gaussian",
        Filter::Lanczos => "Lanczos",
    }
}

fn align_name(align: Align) -> &'static str {
    match align {
        Align::Start => "start",
        Align::Center => "centre",
        Align::End => "end",
    }
}

/// A one-line description of an operation.
fn label(operation: &Operation, locale: &Locale, tools: &Tools) -> String {
    let t = |key| locale.text(key).to_owned();
    match operation {
        Operation::Rotate(turn) => format!("{} {}", t("Rotate"), locale.text(turn_name(*turn))),
        Operation::Flip(Axis::Horizontal) => t("Flip horizontally"),
        Operation::Flip(Axis::Vertical) => t("Flip vertically"),
        Operation::Straighten { .. } => t("Rotate by an angle"),
        Operation::Resize { .. } => t("Resize"),
        Operation::Crop(CropMode::Trim { .. }) => t("Trim borders"),
        Operation::Crop(_) => t("Crop"),
        Operation::Canvas {
            mode: CanvasMode::Border { .. },
            ..
        } => t("Border"),
        Operation::Canvas { .. } => t("Canvas size"),
        Operation::Brightness(_) => t("Brightness"),
        Operation::Contrast(_) => t("Contrast"),
        Operation::Saturation(_) => t("Saturation"),
        Operation::Gamma(_) => t("Gamma"),
        Operation::Hue(_) => t("Hue"),
        Operation::Grayscale => t("Greyscale"),
        Operation::Sepia => t("Sepia"),
        Operation::Invert => t("Negative"),
        Operation::AutoContrast => t("Auto levels"),
        Operation::Blur(_) => t("Blur"),
        Operation::Sharpen { .. } => t("Sharpen"),
        Operation::Flatten(_) => t("Remove transparency"),
        Operation::Tool { id } => tools.label(id).map_or_else(|| id.clone(), str::to_owned),
    }
}

/// The parameter controls of an operation.
fn edit(ui: &mut egui::Ui, operation: &mut Operation, locale: &Locale) {
    let t = |key| locale.text(key);
    match operation {
        Operation::Rotate(turn) => {
            ui.horizontal(|ui| {
                for option in [QuarterTurn::Left, QuarterTurn::Right, QuarterTurn::Half] {
                    ui.selectable_value(turn, option, t(turn_name(option)));
                }
            });
        }
        Operation::Flip(axis) => {
            ui.horizontal(|ui| {
                ui.selectable_value(axis, Axis::Horizontal, t("horizontally"));
                ui.selectable_value(axis, Axis::Vertical, t("vertically"));
            });
        }
        Operation::Straighten { degrees, fill } => {
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(degrees)
                        .range(-180.0..=180.0)
                        .speed(0.1)
                        .suffix("°"),
                );
                ui.label(t("corners"));
                ui.color_edit_button_srgba_unmultiplied(fill);
            });
        }
        Operation::Resize {
            mode,
            filter,
            enlarge,
        } => resize_ui(ui, mode, filter, enlarge, locale),
        Operation::Crop(mode) => crop_ui(ui, mode, locale),
        Operation::Canvas {
            mode,
            colour,
            anchor,
        } => canvas_ui(ui, mode, colour, anchor, locale),
        Operation::Brightness(value)
        | Operation::Contrast(value)
        | Operation::Saturation(value) => {
            ui.add(egui::Slider::new(value, -100.0..=100.0));
        }
        Operation::Gamma(value) => {
            ui.add(egui::Slider::new(value, 0.1..=5.0).logarithmic(true));
        }
        Operation::Hue(value) => {
            ui.add(egui::Slider::new(value, -180.0..=180.0).suffix("°"));
        }
        Operation::Blur(sigma) => {
            ui.add(
                egui::Slider::new(sigma, 0.1..=50.0)
                    .logarithmic(true)
                    .text(t("radius")),
            );
        }
        Operation::Sharpen { sigma, threshold } => {
            ui.add(
                egui::Slider::new(sigma, 0.1..=20.0)
                    .logarithmic(true)
                    .text(t("radius")),
            );
            ui.add(egui::Slider::new(threshold, 0..=50).text(t("threshold")));
        }
        Operation::Flatten(colour) => {
            ui.horizontal(|ui| {
                ui.label(t("background"));
                ui.color_edit_button_srgb(colour);
            });
        }
        Operation::Grayscale
        | Operation::Sepia
        | Operation::Invert
        | Operation::AutoContrast
        | Operation::Tool { .. } => {}
    }
}

fn size_fields(ui: &mut egui::Ui, width: &mut u32, height: &mut u32) {
    ui.add(egui::DragValue::new(width).range(1..=65_535).suffix(" px"));
    ui.label("×");
    ui.add(egui::DragValue::new(height).range(1..=65_535).suffix(" px"));
}

fn resize_ui(
    ui: &mut egui::Ui,
    mode: &mut ResizeMode,
    filter: &mut Filter,
    enlarge: &mut bool,
    locale: &Locale,
) {
    let t = |key| locale.text(key);
    let kinds = [
        (
            "fit in a box",
            ResizeMode::Fit {
                width: 1920,
                height: 1080,
            },
        ),
        (
            "fill a box",
            ResizeMode::Fill {
                width: 1080,
                height: 1080,
            },
        ),
        (
            "exact size",
            ResizeMode::Exact {
                width: 1920,
                height: 1080,
            },
        ),
        ("width", ResizeMode::Width { width: 1920 }),
        ("height", ResizeMode::Height { height: 1080 }),
        ("long side", ResizeMode::LongSide { size: 1920 }),
        ("short side", ResizeMode::ShortSide { size: 1080 }),
        ("percent", ResizeMode::Percent { percent: 50.0 }),
    ];
    let current = kinds
        .iter()
        .position(|(_, kind)| std::mem::discriminant(kind) == std::mem::discriminant(mode))
        .unwrap_or(0);
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(ui.id().with("resize-mode"))
            .selected_text(t(kinds[current].0))
            .show_ui(ui, |ui| {
                for (index, (name, default)) in kinds.iter().enumerate() {
                    if ui.selectable_label(index == current, t(name)).clicked() && index != current
                    {
                        *mode = *default;
                    }
                }
            });
        match mode {
            ResizeMode::Fit { width, height }
            | ResizeMode::Fill { width, height }
            | ResizeMode::Exact { width, height } => size_fields(ui, width, height),
            ResizeMode::Width { width: size }
            | ResizeMode::Height { height: size }
            | ResizeMode::LongSide { size }
            | ResizeMode::ShortSide { size } => {
                ui.add(egui::DragValue::new(size).range(1..=65_535).suffix(" px"));
            }
            ResizeMode::Percent { percent } => {
                ui.add(
                    egui::DragValue::new(percent)
                        .range(1.0..=1000.0)
                        .suffix(" %"),
                );
            }
        }
    });
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(ui.id().with("resize-filter"))
            .selected_text(t(filter_name(*filter)))
            .show_ui(ui, |ui| {
                for option in Filter::ALL {
                    ui.selectable_value(filter, option, t(filter_name(option)));
                }
            });
        ui.checkbox(enlarge, t("enlarge smaller pictures"));
    });
}

fn anchor_ui(ui: &mut egui::Ui, anchor: &mut Anchor, locale: &Locale) {
    let t = |key| locale.text(key);
    ui.horizontal(|ui| {
        ui.label(t("position"));
        for (axis, salt) in [(&mut anchor.x, "x"), (&mut anchor.y, "y")] {
            egui::ComboBox::from_id_salt(ui.id().with(salt))
                .width(70.0)
                .selected_text(t(align_name(*axis)))
                .show_ui(ui, |ui| {
                    for option in [Align::Start, Align::Center, Align::End] {
                        ui.selectable_value(axis, option, t(align_name(option)));
                    }
                });
        }
    });
}

// One form, top to bottom.
#[allow(clippy::too_many_lines)]
fn crop_ui(ui: &mut egui::Ui, mode: &mut CropMode, locale: &Locale) {
    let t = |key| locale.text(key);
    let kinds = [
        (
            "aspect ratio",
            CropMode::Aspect {
                width: 4,
                height: 3,
                anchor: Anchor::CENTER,
            },
        ),
        (
            "rectangle",
            CropMode::Rect {
                x: 0,
                y: 0,
                width: 1000,
                height: 1000,
            },
        ),
        (
            "margins",
            CropMode::Margins {
                left: 10,
                top: 10,
                right: 10,
                bottom: 10,
            },
        ),
        ("trim borders", CropMode::Trim { tolerance: 8 }),
    ];
    let current = kinds
        .iter()
        .position(|(_, kind)| std::mem::discriminant(kind) == std::mem::discriminant(mode))
        .unwrap_or(0);
    egui::ComboBox::from_id_salt(ui.id().with("crop-mode"))
        .selected_text(t(kinds[current].0))
        .show_ui(ui, |ui| {
            for (index, (name, default)) in kinds.iter().enumerate() {
                if ui.selectable_label(index == current, t(name)).clicked() && index != current {
                    *mode = *default;
                }
            }
        });
    match mode {
        CropMode::Aspect {
            width,
            height,
            anchor,
        } => {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(width).range(1..=1000));
                ui.label(":");
                ui.add(egui::DragValue::new(height).range(1..=1000));
            });
            anchor_ui(ui, anchor, locale);
        }
        CropMode::Rect {
            x,
            y,
            width,
            height,
        } => {
            ui.horizontal(|ui| {
                ui.label("x");
                ui.add(egui::DragValue::new(x));
                ui.label("y");
                ui.add(egui::DragValue::new(y));
            });
            ui.horizontal(|ui| size_fields(ui, width, height));
        }
        CropMode::Margins {
            left,
            top,
            right,
            bottom,
        } => {
            ui.horizontal(|ui| {
                for (name, value) in [
                    ("left", left),
                    ("top", top),
                    ("right", right),
                    ("bottom", bottom),
                ] {
                    ui.label(t(name));
                    ui.add(egui::DragValue::new(value).range(0..=65_535));
                }
            });
        }
        CropMode::Trim { tolerance } => {
            ui.add(egui::Slider::new(tolerance, 0..=128).text(t("tolerance")));
        }
    }
}

fn canvas_ui(
    ui: &mut egui::Ui,
    mode: &mut CanvasMode,
    colour: &mut [u8; 4],
    anchor: &mut Anchor,
    locale: &Locale,
) {
    let t = |key| locale.text(key);
    let kinds = [
        ("border", CanvasMode::Border { size: 20 }),
        (
            "size",
            CanvasMode::Size {
                width: 1920,
                height: 1080,
            },
        ),
        (
            "aspect ratio",
            CanvasMode::Aspect {
                width: 1,
                height: 1,
            },
        ),
    ];
    let current = kinds
        .iter()
        .position(|(_, kind)| std::mem::discriminant(kind) == std::mem::discriminant(mode))
        .unwrap_or(0);
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(ui.id().with("canvas-mode"))
            .selected_text(t(kinds[current].0))
            .show_ui(ui, |ui| {
                for (index, (name, default)) in kinds.iter().enumerate() {
                    if ui.selectable_label(index == current, t(name)).clicked() && index != current
                    {
                        *mode = *default;
                    }
                }
            });
        match mode {
            CanvasMode::Border { size } => {
                ui.add(egui::DragValue::new(size).range(1..=10_000).suffix(" px"));
            }
            CanvasMode::Size { width, height } => size_fields(ui, width, height),
            CanvasMode::Aspect { width, height } => {
                ui.add(egui::DragValue::new(width).range(1..=1000));
                ui.label(":");
                ui.add(egui::DragValue::new(height).range(1..=1000));
            }
        }
        ui.color_edit_button_srgba_unmultiplied(colour);
    });
    if !matches!(mode, CanvasMode::Border { .. }) {
        anchor_ui(ui, anchor, locale);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_like_a_file_manager() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn every_operation_has_a_label() {
        let locale = Locale::default();
        for operation in Operation::catalogue() {
            assert!(!label(&operation, &locale, &Tools::default()).is_empty());
        }
    }
}
