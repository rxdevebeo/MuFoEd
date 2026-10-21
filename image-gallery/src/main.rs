//! Standalone review host: the gallery in a window, with folder dialogs.
//!
//! `image-gallery-demo [folder]` opens the folder at start. Environment:
//! `GALLERY_LANGUAGE` (`en`, `ru`), `GALLERY_SETTINGS` (a JSON file the
//! settings are read from and saved to), `GALLERY_SHOT` (write a PNG
//! screenshot after the previews load, then quit).

use image_gallery::{Gallery, GalleryEvent, Locale, Settings};

fn main() -> eframe::Result {
    let code = std::env::var("GALLERY_LANGUAGE").unwrap_or_else(|_| "en".into());
    let locale = Locale::new(&code);
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_title(locale.text("Picture gallery")),
        ..Default::default()
    };
    restrict_backends(&mut options);
    eframe::run_native(
        "Picture gallery",
        options,
        Box::new(move |_| {
            let mut gallery = Gallery::new(locale);
            let settings_path = std::env::var_os("GALLERY_SETTINGS").map(std::path::PathBuf::from);
            if let Some(settings) = settings_path
                .as_ref()
                .and_then(|path| std::fs::read_to_string(path).ok())
                .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
            {
                gallery.set_settings(settings);
            }
            if let Some(folder) = std::env::args_os().nth(1) {
                gallery.open_folder(folder);
            }
            Ok(Box::new(Demo {
                gallery,
                settings_path,
                saved: None,
                shot: std::env::var("GALLERY_SHOT").ok(),
                frames: 0,
            }))
        }),
    )
}

fn restrict_backends(options: &mut eframe::NativeOptions) {
    #[cfg(windows)]
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        use eframe::egui_wgpu::wgpu::Backends;
        setup.instance_descriptor.backends = Backends::from_env().unwrap_or(Backends::DX12);
    }
    #[cfg(not(windows))]
    let _ = options;
}

struct Demo {
    gallery: Gallery,
    settings_path: Option<std::path::PathBuf>,
    saved: Option<Settings>,
    shot: Option<String>,
    frames: u32,
}

impl eframe::App for Demo {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let events = egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ui, |ui| self.gallery.show(ui))
            .inner;
        for event in events {
            self.handle(event);
        }
        self.persist();
        self.shot(ui);
    }
}

impl Demo {
    /// Writes the settings whenever they change.
    fn persist(&mut self) {
        let Some(path) = &self.settings_path else {
            return;
        };
        if self.saved.as_ref() == Some(self.gallery.settings()) {
            return;
        }
        let settings = self.gallery.settings().clone();
        if let Ok(json) = serde_json::to_string_pretty(&settings) {
            let _ = std::fs::write(path, json);
        }
        self.saved = Some(settings);
    }

    fn handle(&mut self, event: GalleryEvent) {
        let start = self.gallery.folder().map(std::path::Path::to_path_buf);
        let dialog = || {
            let dialog = rfd::FileDialog::new();
            match &start {
                Some(folder) => dialog.set_directory(folder),
                None => dialog,
            }
        };
        match event {
            GalleryEvent::PickFolder => {
                if let Some(folder) = dialog().pick_folder() {
                    self.gallery.open_folder(folder);
                }
            }
            GalleryEvent::PickDestination(purpose) => {
                // Cancelling keeps the current destination.
                if let Some(folder) = dialog().pick_folder() {
                    self.gallery.set_destination(purpose, folder);
                }
            }
            GalleryEvent::Reveal(path) => reveal(&path),
            GalleryEvent::OpenInEditor(path) => {
                // The demo has no editor; the system viewer stands in.
                open_with_system(&path);
            }
            GalleryEvent::FolderOpened(_) | GalleryEvent::Written(_) => {}
        }
    }

    fn shot(&mut self, ui: &mut egui::Ui) {
        let Some(path) = self.shot.clone() else {
            return;
        };
        self.frames += 1;
        if self.frames == 90 {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let screenshot = ui.ctx().input(|input| {
            input.events.iter().find_map(|event| {
                if let egui::Event::Screenshot { image, .. } = event {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let Some(image) = screenshot {
            let mut bytes = Vec::with_capacity(image.pixels.len() * 4);
            for pixel in &image.pixels {
                bytes.extend_from_slice(&pixel.to_array());
            }
            let saved = u32::try_from(image.size[0])
                .ok()
                .zip(u32::try_from(image.size[1]).ok())
                .and_then(|(width, height)| image::RgbaImage::from_raw(width, height, bytes))
                .map(|picture| picture.save(&path));
            if !matches!(saved, Some(Ok(()))) {
                eprintln!("could not write the screenshot to {path}");
            }
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ui.ctx().request_repaint();
    }
}

fn reveal(path: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer")
        .arg("/select,")
        .arg(path)
        .spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(folder) = path.parent() {
        let _ = std::process::Command::new("xdg-open").arg(folder).spawn();
    }
}

fn open_with_system(path: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}
