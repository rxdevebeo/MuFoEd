//! On-demand gallery and file manager hosts. Instances retain navigation and
//! extracted archive members until application exit, including after tab closure.
use crate::file_tabs::FileTab;
use std::{
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub(crate) struct Browsers {
    gallery: Option<image_gallery::Gallery>,
    files: Option<file_manager::FileManager>,
    gallery_open: bool,
    files_open: bool,
    saved: [Option<Vec<u8>>; 2],
    pub notice: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
struct GalleryPreferences {
    settings: image_gallery::Settings,
    folder: Option<PathBuf>,
}

pub(crate) enum Event {
    Open(PathBuf),
    System(PathBuf),
    Text(PathBuf),
    Reveal(PathBuf),
}

impl Browsers {
    pub(crate) fn open(&mut self, key: FileTab, language: &str) {
        match key {
            FileTab::Gallery => {
                if self.gallery.is_none() {
                    let preferences: GalleryPreferences = load("gallery.json");
                    let mut gallery =
                        image_gallery::Gallery::new(image_gallery::Locale::new(language));
                    gallery.set_settings(preferences.settings);
                    gallery.set_background_processor(|remover, image| {
                        let settings = remover.settings();
                        let weights =
                            std::fs::metadata(settings.model_path()).map_or(0, |m| m.len());
                        let bytes = weights
                            .saturating_mul(4)
                            .saturating_add(1 << 30)
                            .saturating_add(
                                u64::from(image.width()) * u64::from(image.height()) * 32,
                            );
                        let mut task = mufoed_model_runtime::Task::new("gallery", "BiRefNet");
                        task.device = if settings.gpu { "auto" } else { "cpu" }.into();
                        task.memory = mufoed_model_runtime::Memory {
                            ram: Some(bytes),
                            vram: Some(if settings.gpu { bytes } else { 0 }),
                        };
                        let error = |e: mufoed_model_runtime::Error| {
                            image_gallery::Error::Invalid(e.to_string())
                        };
                        let lease = mufoed_model_runtime::Runtime::global()
                            .enqueue(task)
                            .map_err(error)?
                            .wait(
                                &std::sync::atomic::AtomicBool::new(false),
                                std::time::Duration::from_secs(120),
                            )
                            .map_err(error)?;
                        let result = remover.remove(&image);
                        remover.shut_down();
                        drop(lease);
                        result
                    });
                    if let Some(folder) = preferences.folder.filter(|p| p.is_dir()) {
                        gallery.open_folder(folder);
                    }
                    self.gallery = Some(gallery);
                }
                self.gallery_open = true;
            }
            FileTab::Files => {
                self.files.get_or_insert_with(|| {
                    let mut files = file_manager::FileManager::new(
                        file_manager::Locale::new(language),
                        load("files.json"),
                    );
                    files.set_document_provider(std::sync::Arc::new(
                        crate::document_preview::Documents,
                    ));
                    files
                });
                self.files_open = true;
            }
            _ => {}
        }
    }

    pub(crate) fn live(&self) -> impl Iterator<Item = FileTab> {
        [
            self.gallery_open.then_some(FileTab::Gallery),
            self.files_open.then_some(FileTab::Files),
        ]
        .into_iter()
        .flatten()
    }

    pub(crate) fn busy(&self, key: FileTab) -> bool {
        match key {
            FileTab::Gallery => self
                .gallery
                .as_ref()
                .is_some_and(|g| g.can_execute(image_gallery::Command::CancelJob)),
            FileTab::Files => self
                .files
                .as_ref()
                .is_some_and(file_manager::FileManager::is_busy),
            _ => false,
        }
    }

    pub(crate) fn preview_file(&mut self, path: &Path, quick: bool) {
        if let Some(files) = &mut self.files {
            if let Err(error) = files.view_file(path, quick) {
                self.notice = Some(error);
            }
        }
    }

    pub(crate) fn pause_media(&mut self) {
        if let Some(files) = &mut self.files {
            files.pause_media();
        }
    }

    pub(crate) fn close(&mut self, key: FileTab) -> bool {
        if self.busy(key) {
            return false;
        }
        match key {
            FileTab::Gallery => self.gallery_open = false,
            FileTab::Files => {
                self.pause_media();
                self.files_open = false;
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn reveal(&mut self, path: &Path, language: &str) {
        self.open(FileTab::Files, language);
        if let (Some(files), Some(folder)) = (&mut self.files, path.parent()) {
            files.open_folder(folder);
        }
    }

    pub(crate) fn poll(&mut self, ctx: &egui::Context) {
        if let Some(gallery) = &mut self.gallery {
            gallery.poll(ctx);
        }
        if let Some(files) = &mut self.files {
            files.poll(ctx);
        }
        self.persist();
    }

    fn persist(&mut self) {
        let gallery = self.gallery.as_ref().and_then(|g| {
            serde_json::to_vec_pretty(&GalleryPreferences {
                settings: g.settings().clone(),
                folder: g.folder().map(Path::to_path_buf),
            })
            .ok()
        });
        let files = self
            .files
            .as_ref()
            .and_then(|f| serde_json::to_vec_pretty(&f.settings()).ok());
        for (index, (name, data)) in [("gallery.json", gallery), ("files.json", files)]
            .into_iter()
            .enumerate()
        {
            if let Some(data) = data.filter(|data| self.saved[index].as_ref() != Some(data)) {
                match save(name, &data) {
                    Ok(()) => self.saved[index] = Some(data),
                    Err(error) => self.notice = Some(error.to_string()),
                }
            }
        }
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui, key: FileTab, language: &str) -> Vec<Event> {
        if let Some(notice) = self.notice.clone() {
            egui::Panel::top("browser-notice").show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(notice);
                    if ui.small_button("×").clicked() {
                        self.notice = None;
                    }
                });
            });
        }
        match key {
            FileTab::Gallery => {
                let Some(gallery) = &mut self.gallery else {
                    return Vec::new();
                };
                gallery.set_locale(image_gallery::Locale::new(language));
                let events = gallery.show(ui);
                let mut result = Vec::new();
                for event in events {
                    use image_gallery::GalleryEvent as G;
                    let dialog = || {
                        let dialog = rfd::FileDialog::new();
                        gallery
                            .folder()
                            .map_or(dialog.clone(), |folder| dialog.set_directory(folder))
                    };
                    match event {
                        G::OpenInEditor(path) => result.push(Event::Open(path)),
                        G::Reveal(path) => result.push(Event::Reveal(path)),
                        G::PickFolder => {
                            if let Some(folder) = dialog().pick_folder() {
                                gallery.open_folder(folder);
                            }
                        }
                        G::PickDestination(purpose) => {
                            if let Some(folder) = dialog().pick_folder() {
                                gallery.set_destination(purpose, folder);
                            }
                        }
                        G::PickModel => {
                            if let Some(path) =
                                dialog().add_filter("BiRefNet ONNX", &["onnx"]).pick_file()
                            {
                                gallery.set_background_model(path);
                            }
                        }
                        G::FolderOpened(_) | G::Written(_) => {}
                    }
                }
                result
            }
            FileTab::Files => {
                let Some(files) = &mut self.files else {
                    return Vec::new();
                };
                files.set_locale(file_manager::Locale::new(language));
                files
                    .show(ui)
                    .into_iter()
                    .map(|event| match event {
                        file_manager::Event::OpenInEditor(path) => Event::Open(path),
                        file_manager::Event::OpenWithSystem(path) => Event::System(path),
                        file_manager::Event::EditText(path) => Event::Text(path),
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }
}

fn load<T: serde::de::DeserializeOwned + Default>(name: &str) -> T {
    crate::settings::config_dir()
        .and_then(|d| std::fs::read(d.join(name)).ok())
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}
fn save(name: &str, data: &[u8]) -> std::io::Result<()> {
    let Some(directory) = crate::settings::config_dir() else {
        return Ok(());
    };
    std::fs::create_dir_all(&directory)?;
    let mut file = tempfile::NamedTempFile::new_in(&directory)?;
    file.write_all(data)?;
    file.persist(directory.join(name)).map_err(|e| e.error)?;
    Ok(())
}

pub(crate) fn open_system(path: &Path, text: bool) -> std::io::Result<()> {
    #[cfg(windows)]
    let mut command = std::process::Command::new(if text { "notepad.exe" } else { "explorer.exe" });
    #[cfg(target_os = "macos")]
    let mut command = {
        let _ = text;
        std::process::Command::new("open")
    };
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let mut command = {
        let _ = text;
        std::process::Command::new("xdg-open")
    };
    command.arg(path).spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gallery_reopens_with_selection_and_routes_to_editor() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sample.png");
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[0, 0, 0, 255])
                .unwrap();
        }
        std::fs::write(&path, png).unwrap();
        let mut gallery = image_gallery::Gallery::new(image_gallery::Locale::new("en"));
        gallery.open_folder(directory.path());
        let ctx = egui::Context::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut time = 0.0;
        while gallery.selection().is_empty() {
            time += 0.1;
            let _ = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    ..Default::default()
                },
                |_| {
                    gallery.poll(&ctx);
                    gallery.select(std::slice::from_ref(&path));
                },
            );
            assert!(
                std::time::Instant::now() < deadline,
                "gallery listing did not arrive"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let mut host = Browsers {
            gallery: Some(gallery),
            gallery_open: true,
            ..Browsers::default()
        };
        assert!(host.close(FileTab::Gallery));
        host.open(FileTab::Gallery, "ru");
        assert!(host.files.is_none());
        host.gallery
            .as_mut()
            .unwrap()
            .execute(image_gallery::Command::OpenInEditor);
        let mut events = Vec::new();
        let _ = ctx.run_ui(
            egui::RawInput {
                time: Some(time + 1.0),
                ..Default::default()
            },
            |ui| {
                events.extend(host.show(ui, FileTab::Gallery, "ru"));
            },
        );
        assert!(events
            .iter()
            .any(|event| matches!(event, Event::Open(p) if p == &path)));
        assert_eq!(host.gallery.as_ref().unwrap().selection(), vec![path]);
        host.gallery
            .as_mut()
            .unwrap()
            .execute(image_gallery::Command::FlipHorizontal);
        assert!(host.busy(FileTab::Gallery));
        assert!(!host.close(FileTab::Gallery));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while host.busy(FileTab::Gallery) {
            time += 1.0;
            let _ = ctx.run_ui(
                egui::RawInput {
                    time: Some(time + 1.0),
                    ..Default::default()
                },
                |_| {
                    host.gallery.as_mut().unwrap().poll(&ctx);
                },
            );
            assert!(
                std::time::Instant::now() < deadline,
                "hidden gallery job did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(host.close(FileTab::Gallery));
    }

    #[test]
    fn default_has_no_instances_or_open_tabs() {
        let host = Browsers::default();
        assert!(host.gallery.is_none());
        assert!(host.files.is_none());
        assert_eq!(host.live().count(), 0);
    }
    #[test]
    fn utility_tabs_close_independently() {
        let mut host = Browsers {
            gallery_open: true,
            files_open: true,
            ..Browsers::default()
        };
        assert!(host.close(FileTab::Gallery));
        assert_eq!(host.live().collect::<Vec<_>>(), vec![FileTab::Files]);
        assert!(!host.close(FileTab::Document(1)));
        assert!(host.close(FileTab::Files));
        assert_eq!(host.live().count(), 0);
    }
}
