//! The quick view (Ctrl+Q, in the other panel) and the viewer (F3).

// Row counts and texture sizes are small positive values.
#![allow(clippy::cast_precision_loss)]

use std::path::PathBuf;

use egui::TextureHandle;
use image_gallery::gallery::ModelPanel;

use super::{FileManager, Location, Panel};
use crate::archive::{self, TempFolder};
use crate::preview::{self, Content, Loading};
use crate::Locale;

/// A loaded (or loading) preview of one file.
pub(crate) struct Preview {
    /// What it shows: a path, or `archive|member`.
    key: String,
    title: String,
    loading: Option<Loading>,
    content: Option<Content>,
    lines: Vec<String>,
    texture: Option<TextureHandle>,
    /// The extracted copy of an archive member.
    _temporary: Option<TempFolder>,
}

/// The viewer's state.
pub(crate) struct Viewer {
    preview: Preview,
    hex: bool,
    wrap: bool,
}

/// Quick view and viewer state.
#[derive(Default)]
pub(crate) struct ViewState {
    /// The quick view is on.
    pub(crate) quick: bool,
    /// Text to put on the clipboard this frame.
    pub(crate) copied: Option<String>,
    quick_preview: Option<Preview>,
    pub(crate) viewer: Option<Viewer>,
    model: ModelPanel,
}

/// Largest picture side of the quick view and the viewer.
const QUICK_SIDE: u32 = 1024;
const VIEWER_SIDE: u32 = 4096;

impl Preview {
    /// Starts loading the row `entry` of a panel at `location`.
    fn start(
        location: &Location,
        entry: &crate::entry::Entry,
        side: u32,
        wake: impl Fn() + Send + 'static,
    ) -> Self {
        let (key, path, temporary) = match location {
            Location::Archive { file, .. } => {
                let member = entry.path.to_string_lossy().into_owned();
                let key = format!("{}|{member}", file.display());
                match archive::extract_to_temporary(file, &member) {
                    Ok((path, folder)) => (key, path, Some(folder)),
                    Err(error) => {
                        return Self {
                            key,
                            title: entry.name.clone(),
                            loading: None,
                            content: Some(Content::Failed(error.to_string())),
                            lines: Vec::new(),
                            texture: None,
                            _temporary: None,
                        };
                    }
                }
            }
            _ => (entry.path.display().to_string(), entry.path.clone(), None),
        };
        Self {
            key,
            title: entry.name.clone(),
            loading: Some(Loading::start(&path, side, wake)),
            content: None,
            lines: Vec::new(),
            texture: None,
            _temporary: temporary,
        }
    }

    fn poll(&mut self) {
        if let Some(content) = self.loading.as_ref().and_then(Loading::poll) {
            if let Content::Text { text, .. } = &content {
                self.lines = text
                    .lines()
                    .map(|line| line.replace('\t', "    "))
                    .collect();
            }
            self.content = Some(content);
            self.loading = None;
            self.texture = None;
        }
    }
}

/// The key of a panel row, as previews name it.
fn key_of(location: &Location, entry: &crate::entry::Entry) -> String {
    match location {
        Location::Archive { file, .. } => format!("{}|{}", file.display(), entry.path.display()),
        _ => entry.path.display().to_string(),
    }
}

fn model_path(preview: &Preview) -> Option<PathBuf> {
    match &preview.content {
        Some(Content::Model(path)) => Some(path.clone()),
        _ => None,
    }
}

/// Draws a preview's content.
fn draw(
    ui: &mut egui::Ui,
    preview: &mut Preview,
    model: &mut ModelPanel,
    locale: &Locale,
    hex: bool,
    wrap: bool,
) {
    preview.poll();
    let Some(content) = &preview.content else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(locale.text("Reading…"));
        });
        return;
    };
    match content {
        Content::Failed(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        Content::Folder(usage) => {
            ui.heading(&preview.title);
            ui.label(locale.format(
                "{} files, {} folders, {}",
                &[
                    usage.files.to_string(),
                    usage.folders.to_string(),
                    crate::entry::human_size(usage.bytes),
                ],
            ));
            ui.monospace(format!("{} B", crate::entry::grouped(usage.bytes)));
        }
        Content::Model(path) => {
            let path = path.clone();
            model.show(ui, &path, &image_gallery::Locale::new(locale.code()));
        }
        Content::Picture {
            size,
            rgba,
            description,
        } => {
            ui.weak(description);
            let texture = preview.texture.get_or_insert_with(|| {
                let image = egui::ColorImage::from_rgba_unmultiplied(*size, rgba);
                ui.ctx()
                    .load_texture("file-manager-preview", image, egui::TextureOptions::LINEAR)
            });
            let available = ui.available_size();
            let picture = texture.size_vec2();
            let scale = (available.x / picture.x)
                .min(available.y / picture.y)
                .min(1.0)
                .max(0.01);
            ui.centered_and_justified(|ui| {
                ui.image((texture.id(), picture * scale));
            });
        }
        Content::Hex { bytes, size } => draw_hex(ui, bytes, *size, locale),
        Content::Text {
            encoding,
            truncated,
            ..
        } => {
            if hex {
                let bytes: Vec<u8> = preview.lines.join("\n").into_bytes();
                draw_hex(ui, &bytes, bytes.len() as u64, locale);
                return;
            }
            ui.weak(format!(
                "{encoding}{}",
                if *truncated {
                    locale.text(" · the start of the file")
                } else {
                    ""
                }
            ));
            let lines = &preview.lines;
            if wrap {
                egui::ScrollArea::vertical()
                    .id_salt("file-manager-text-wrapped")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for line in lines.iter().take(5000) {
                            ui.add(egui::Label::new(egui::RichText::new(line).monospace()).wrap());
                        }
                    });
            } else {
                let height = ui.text_style_height(&egui::TextStyle::Monospace);
                egui::ScrollArea::both()
                    .id_salt("file-manager-text")
                    .auto_shrink([false, false])
                    .show_rows(ui, height, lines.len(), |ui, range| {
                        for line in &lines[range] {
                            ui.add(
                                egui::Label::new(egui::RichText::new(line).monospace()).extend(),
                            );
                        }
                    });
            }
        }
    }
}

fn draw_hex(ui: &mut egui::Ui, bytes: &[u8], size: u64, locale: &Locale) {
    ui.weak(locale.format("{} bytes", &[crate::entry::grouped(size)]));
    let height = ui.text_style_height(&egui::TextStyle::Monospace);
    let rows = bytes.len().div_ceil(16);
    egui::ScrollArea::both()
        .id_salt("file-manager-hex")
        .auto_shrink([false, false])
        .show_rows(ui, height, rows, |ui, range| {
            let start = range.start * 16;
            let end = (range.end * 16).min(bytes.len());
            for (offset, line) in preview::hex_lines(&bytes[start..end]).enumerate() {
                // The dump restarts its offsets at the slice: put them back.
                let absolute = format!("{:08X}{}", start + offset * 16, &line[8..]);
                ui.add(egui::Label::new(egui::RichText::new(absolute).monospace()).extend());
            }
        });
}

impl FileManager {
    /// The quick view in the inactive panel's place.
    pub(super) fn quick_view_ui(&mut self, ui: &mut egui::Ui, locale: &Locale) {
        let panel = &self.panels[self.active];
        let Some(entry) = panel
            .current()
            .filter(|entry| !Panel::is_up(entry))
            .cloned()
        else {
            ui.weak(locale.text("Quick view: put the cursor on a file"));
            return;
        };
        let key = key_of(&panel.location, &entry);
        if self
            .view
            .quick_preview
            .as_ref()
            .is_none_or(|preview| preview.key != key)
        {
            let location = panel.location.clone();
            self.view.quick_preview = Some(Preview::start(
                &location,
                &entry,
                QUICK_SIDE,
                self.wake.function(),
            ));
            if self
                .view
                .quick_preview
                .as_ref()
                .and_then(model_path)
                .is_none()
            {
                self.view.model.close();
            }
        }
        ui.strong(&entry.name);
        ui.separator();
        if let Some(preview) = self.view.quick_preview.as_mut() {
            draw(ui, preview, &mut self.view.model, locale, false, false);
        }
    }

    /// F3: the viewer over the panels.
    pub(super) fn view_current(&mut self) {
        let panel = &self.panels[self.active];
        let Some(entry) = panel
            .current()
            .filter(|entry| !Panel::is_up(entry))
            .cloned()
        else {
            return;
        };
        let preview = Preview::start(&panel.location, &entry, VIEWER_SIDE, self.wake.function());
        self.view.viewer = Some(Viewer {
            preview,
            hex: false,
            wrap: false,
        });
    }

    pub(super) fn viewer_ui(&mut self, ui: &mut egui::Ui, locale: &Locale) {
        let mut close = ui.input(|input| {
            input.key_pressed(egui::Key::Escape) || input.key_pressed(egui::Key::F3)
        });
        let Some(viewer) = self.view.viewer.as_mut() else {
            return;
        };
        ui.horizontal(|ui| {
            if ui.button(locale.text("← Close (Esc)")).clicked() {
                close = true;
            }
            ui.strong(&viewer.preview.title);
            ui.separator();
            ui.selectable_value(&mut viewer.hex, false, locale.text("Text"));
            ui.selectable_value(&mut viewer.hex, true, locale.text("Hex"));
            ui.checkbox(&mut viewer.wrap, locale.text("Wrap lines"));
            if ui.button(locale.text("Copy text")).clicked() {
                self.view.copied = Some(viewer.preview.lines.join("\n"));
            }
        });
        ui.separator();
        let (hex, wrap) = (viewer.hex, viewer.wrap);
        draw(
            ui,
            &mut viewer.preview,
            &mut self.view.model,
            locale,
            hex,
            wrap,
        );
        if let Some(text) = self.view.copied.take() {
            ui.ctx().copy_text(text);
        }
        if close {
            self.view.viewer = None;
            self.view.model.close();
        }
    }
}
