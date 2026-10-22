//! The quick view (Ctrl+Q, in the other panel) and the viewer (F3).

// Row counts and texture sizes are small positive values.
#![allow(clippy::cast_precision_loss)]

use std::{path::PathBuf, sync::Arc};

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
    path: PathBuf,
    document: Option<super::document_view::View>,
    markdown: Option<super::markdown::Markdown>,
    player: Option<crate::media::Player>,
    loading: Option<Loading>,
    content: Option<Content>,
    lines: Vec<String>,
    texture: Option<TextureHandle>,
    /// The extracted copy of an archive member.
    _temporary: Option<Arc<TempFolder>>,
}

/// The viewer's state.
pub(crate) struct Viewer {
    preview: Preview,
    hex: bool,
    wrap: bool,
    formatted: bool,
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
    pub(super) provider: Option<Arc<dyn crate::documents::Provider>>,
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
        provider: Option<Arc<dyn crate::documents::Provider>>,
        wake: impl Fn() + Send + 'static,
    ) -> Self {
        let (key, path, temporary) = match location {
            Location::Archive { file, .. } => {
                let member = entry.path.to_string_lossy().into_owned();
                let key = format!("{}|{member}", file.display());
                match archive::extract_to_temporary(file, &member) {
                    Ok((path, folder)) => (key, path, Some(Arc::new(folder))),
                    Err(error) => {
                        return Self {
                            key,
                            title: entry.name.clone(),
                            path: entry.path.clone(),
                            document: None,
                            markdown: None,
                            player: None,
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
        let (document, loading) = if let Some(provider) = provider.filter(|p| p.supports(&path)) {
            (
                Some(super::document_view::View::start(
                    path.clone(),
                    provider,
                    temporary.clone(),
                    side == VIEWER_SIDE,
                    wake,
                )),
                None,
            )
        } else {
            (None, Some(Loading::start(&path, side, wake)))
        };
        Self {
            key,
            title: entry.name.clone(),
            path: path.clone(),
            document,
            markdown: None,
            player: None,
            loading,
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
    formatted: bool,
) {
    if let Some(document) = &mut preview.document {
        document.show(ui, locale);
        return;
    }
    preview.poll();
    let Some(content) = &preview.content else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(locale.text("Reading…"));
        });
        return;
    };
    match content {
        Content::Media(info) => {
            let autoplay = preview.player.is_none();
            let player = preview
                .player
                .get_or_insert_with(|| crate::media::Player::new(info.clone()));
            player.show(ui, locale, autoplay);
        }
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
                .clamp(0.01, 1.0);
            ui.centered_and_justified(|ui| {
                ui.image((texture.id(), picture * scale));
            });
        }
        Content::Hex { bytes, size } => draw_hex(ui, bytes, *size, locale),
        Content::Text {
            encoding,
            truncated,
            text,
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
            if formatted && is_markdown(&preview.path) {
                preview
                    .markdown
                    .get_or_insert_with(|| super::markdown::Markdown::new(text, &preview.path))
                    .show(ui);
                return;
            }
            draw_text(ui, &preview.lines, wrap);
        }
    }
}

fn draw_text(ui: &mut egui::Ui, lines: &[String], wrap: bool) {
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
                    ui.add(egui::Label::new(egui::RichText::new(line).monospace()).extend());
                }
            });
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
    /// Drop the previous player's ownership as soon as the cursor changes,
    /// including parent rows, empty folders and changes made while not drawing.
    pub(super) fn sync_quick_preview(&mut self) {
        let key = if self.view.quick && self.view.viewer.is_none() {
            let panel = &self.panels[self.active];
            panel
                .current()
                .filter(|entry| !Panel::is_up(entry))
                .map(|entry| key_of(&panel.location, entry))
        } else {
            None
        };
        if self
            .view
            .quick_preview
            .as_ref()
            .is_some_and(|preview| Some(preview.key.as_str()) != key.as_deref())
        {
            self.view.clear_quick();
            self.view.model.close();
        }
    }

    /// Opens a local file in the full viewer or in the opposite panel's quick view.
    /// # Errors
    /// Returns a diagnostic if the source is missing or cannot be inspected.
    pub fn view_file(&mut self, path: impl Into<PathBuf>, quick: bool) -> Result<(), String> {
        let path = std::path::absolute(path.into()).map_err(|e| e.to_string())?;
        let entry = crate::entry::Entry::from_path(&path).ok_or("File metadata unavailable")?;
        let folder = path.parent().ok_or("No parent folder")?.to_path_buf();
        let location = Location::Folder(folder);
        self.panels[self.active].go(
            location.clone(),
            Some(entry.name.clone()),
            &self.wake.function(),
        );
        self.view.pause_media();
        self.view.viewer = None;
        self.view.clear_quick();
        let preview = Preview::start(
            &location,
            &entry,
            if quick { QUICK_SIDE } else { VIEWER_SIDE },
            self.view.provider.clone(),
            self.wake.function(),
        );
        self.view.quick = quick;
        if quick {
            self.view.quick_preview = Some(preview);
        } else {
            self.view.viewer = Some(Viewer {
                preview,
                hex: false,
                wrap: false,
                formatted: true,
            });
        }
        Ok(())
    }

    /// The quick view in the inactive panel's place.
    pub(super) fn quick_view_ui(&mut self, ui: &mut egui::Ui, locale: &Locale) {
        self.sync_quick_preview();
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
            self.view.clear_quick();
            self.view.quick_preview = Some(Preview::start(
                &location,
                &entry,
                QUICK_SIDE,
                self.view.provider.clone(),
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
            draw(
                ui,
                preview,
                &mut self.view.model,
                locale,
                false,
                false,
                true,
            );
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
        let preview = Preview::start(
            &panel.location,
            &entry,
            VIEWER_SIDE,
            self.view.provider.clone(),
            self.wake.function(),
        );
        self.view.clear_quick();
        self.view.viewer = Some(Viewer {
            preview,
            hex: false,
            wrap: false,
            formatted: true,
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
            if matches!(
                viewer.preview.content,
                Some(Content::Text { .. } | Content::Hex { .. })
            ) {
                if is_markdown(&viewer.preview.path)
                    && ui
                        .selectable_label(viewer.formatted && !viewer.hex, locale.text("Formatted"))
                        .clicked()
                {
                    viewer.formatted = true;
                    viewer.hex = false;
                }
                if ui
                    .selectable_label(!viewer.formatted && !viewer.hex, locale.text("Text"))
                    .clicked()
                {
                    viewer.formatted = false;
                    viewer.hex = false;
                }
                if ui
                    .selectable_label(viewer.hex, locale.text("Hex"))
                    .clicked()
                {
                    viewer.hex = true;
                    viewer.formatted = false;
                }
                ui.checkbox(&mut viewer.wrap, locale.text("Wrap lines"));
                if ui.button(locale.text("Copy text")).clicked() {
                    self.view.copied = Some(viewer.preview.lines.join("\n"));
                }
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
            viewer.formatted,
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

fn is_markdown(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["md", "markdown", "mdown", "mkd"]
            .iter()
            .any(|x| e.eq_ignore_ascii_case(x))
    })
}
impl ViewState {
    pub(super) fn pause_media(&mut self) {
        for preview in [
            self.quick_preview.as_mut(),
            self.viewer.as_mut().map(|v| &mut v.preview),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(player) = &mut preview.player {
                player.pause();
            }
        }
    }
    pub(super) fn clear_quick(&mut self) {
        self.quick_preview = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "oxideav")]
    #[test]
    fn changing_quick_selection_stops_oxideav_worker() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("sound.wav");
        crate::media::silent_wav(&path);
        let text = root.path().join("other.txt");
        std::fs::write(&text, "text").unwrap();
        let mut manager = FileManager::new(
            Locale::new("en"),
            crate::Settings {
                left: Some(root.path().into()),
                right: Some(root.path().into()),
                ..Default::default()
            },
        );
        let ctx = egui::Context::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while manager.panels[0].is_loading() {
            manager.poll(&ctx);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let audio = manager.panels[0]
            .rows
            .iter()
            .position(|r| r.path == path)
            .unwrap();
        let other = manager.panels[0]
            .rows
            .iter()
            .position(|r| r.path == text)
            .unwrap();
        for destination in [Some(other), Some(0), None] {
            manager.panels[0].cursor = audio;
            manager.view.quick = true;
            let panel = &manager.panels[0];
            let mut preview = Preview::start(
                &panel.location,
                panel.current().unwrap(),
                QUICK_SIDE,
                None,
                || {},
            );
            let mut player = crate::media::Player::new(crate::media::probe(&path).unwrap());
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                player.show(ui, &Locale::new("en"), true);
            });
            let alive = player.worker_alive().unwrap();
            preview.player = Some(player);
            manager.view.quick_preview = Some(preview);
            if let Some(index) = destination {
                manager.panels[0].cursor = index;
            } else {
                manager.panels[0].rows.clear();
            }
            manager.sync_quick_preview();
            assert!(manager.view.quick_preview.is_none());
            assert!(!alive.load(std::sync::atomic::Ordering::Acquire));
        }
    }
    #[cfg(all(windows, not(feature = "oxideav")))]
    #[test]
    fn changing_quick_view_selection_closes_native_player() {
        let directory = tempfile::tempdir().unwrap();
        let audio_path = directory.path().join("sound.wav");
        crate::native_audio::silent_wav(&audio_path);
        let text_path = directory.path().join("other.txt");
        std::fs::write(&text_path, "text").unwrap();
        let mut manager = FileManager::new(
            Locale::new("en"),
            crate::Settings {
                left: Some(directory.path().into()),
                right: Some(directory.path().into()),
                ..Default::default()
            },
        );
        let ctx = egui::Context::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while manager.panels[0].is_loading() {
            manager.poll(&ctx);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let audio_index = manager.panels[0]
            .rows
            .iter()
            .position(|r| r.path == audio_path)
            .unwrap();
        let text_index = manager.panels[0]
            .rows
            .iter()
            .position(|r| r.path == text_path)
            .unwrap();
        for destination in [Some(text_index), Some(0), None] {
            manager.panels[0].cursor = audio_index;
            manager.view.quick = true;
            let panel = &manager.panels[0];
            let mut preview = Preview::start(
                &panel.location,
                panel.current().unwrap(),
                QUICK_SIDE,
                None,
                || {},
            );
            let mut player = crate::media::Player::new(crate::media::probe(&audio_path).unwrap());
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                player.show(ui, &Locale::new("en"), true);
            });
            let handle = player.native_handle().expect("native player created");
            preview.player = Some(player);
            manager.view.quick_preview = Some(preview);
            if let Some(index) = destination {
                manager.panels[0].cursor = index;
            } else {
                manager.panels[0].rows.clear();
            }
            manager.sync_quick_preview();
            assert!(manager.view.quick_preview.is_none());
            assert!(handle.PlaybackSession().is_err());
        }
    }
    #[test]
    fn f3_opens_and_closes_viewer_and_quick_view_releases_preview() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("README.md");
        std::fs::write(&path, "# Heading\n\n**Bold**").unwrap();
        let mut manager = FileManager::new(
            Locale::new("en"),
            crate::Settings {
                left: Some(directory.path().into()),
                right: Some(directory.path().into()),
                ..Default::default()
            },
        );
        let ctx = egui::Context::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            manager.poll(&ctx);
            if let Some(index) = manager.panels[0].rows.iter().position(|r| r.path == path) {
                manager.panels[0].cursor = index;
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        for (key, ctrl, expected_viewer) in [
            (egui::Key::F3, false, true),
            (egui::Key::F3, false, false),
            (egui::Key::Q, true, false),
            (egui::Key::Q, true, false),
        ] {
            let modifiers = egui::Modifiers {
                ctrl,
                command: ctrl,
                ..Default::default()
            };
            let input = egui::RawInput {
                modifiers,
                events: vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                manager.show(ui);
            });
            assert_eq!(manager.view.viewer.is_some(), expected_viewer);
        }
        assert!(!manager.view.quick);
        assert!(manager.view.quick_preview.is_none());
    }
}
