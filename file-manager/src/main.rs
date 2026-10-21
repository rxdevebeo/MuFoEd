//! Standalone review host: the file manager in a window.
//!
//! `file-manager-demo [folder]`. Environment: `FILE_MANAGER_LANGUAGE`
//! (`en`, `ru`), `FILE_MANAGER_SETTINGS` (a JSON file the settings are read
//! from and saved to), `FILE_MANAGER_SHOT` (write a PNG screenshot after a
//! moment, then quit).

use file_manager::{Event, FileManager, Locale, Settings};

fn main() -> eframe::Result {
    let code = std::env::var("FILE_MANAGER_LANGUAGE").unwrap_or_else(|_| "en".into());
    let locale = Locale::new(&code);
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1360.0, 860.0])
            .with_title(locale.text("File manager")),
        ..Default::default()
    };
    restrict_backends(&mut options);
    eframe::run_native(
        "File manager",
        options,
        Box::new(move |_| {
            let settings_path =
                std::env::var_os("FILE_MANAGER_SETTINGS").map(std::path::PathBuf::from);
            let settings: Settings = settings_path
                .as_ref()
                .and_then(|path| std::fs::read_to_string(path).ok())
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
            let mut manager = FileManager::new(locale, settings.clone());
            if let Some(folder) = std::env::args_os().nth(1) {
                manager.open_folder(folder);
            }
            Ok(Box::new(Demo {
                manager,
                settings_path,
                saved: Some(settings),
                shot: std::env::var("FILE_MANAGER_SHOT").ok(),
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
    manager: FileManager,
    settings_path: Option<std::path::PathBuf>,
    saved: Option<Settings>,
    shot: Option<String>,
    frames: u32,
}

impl eframe::App for Demo {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let events = egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(4))
            .show(ui, |ui| self.manager.show(ui))
            .inner;
        for event in events {
            match event {
                // The demo has no editor: the system's programs stand in.
                Event::OpenInEditor(path) | Event::OpenWithSystem(path) => open_with_system(&path),
                Event::EditText(path) => edit_text(&path),
            }
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
        let settings = self.manager.settings();
        if self.saved.as_ref() == Some(&settings) {
            return;
        }
        if let Ok(json) = serde_json::to_string_pretty(&settings) {
            let _ = std::fs::write(path, json);
        }
        self.saved = Some(settings);
    }

    fn shot(&mut self, ui: &mut egui::Ui) {
        let Some(path) = self.shot.clone() else {
            return;
        };
        self.frames += 1;
        if self.frames == 60 {
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

fn open_with_system(path: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", ""])
        .arg(path)
        .spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

fn edit_text(path: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("notepad").arg(path).spawn();
    #[cfg(not(windows))]
    {
        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| "xdg-open".into());
        let _ = std::process::Command::new(editor).arg(path).spawn();
    }
}
