//! Standalone review host. The library itself has no file dialogs or I/O.

use chem_editor::{ChemDocument, ChemEditor, Locale};
use egui::{Color32, RichText};
use std::io::Read as _;

fn main() -> eframe::Result {
    let code = std::env::var("CHEM_LANGUAGE").unwrap_or_else(|_| "en".into());
    let locale = build_locale(&code);
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1240.0, 800.0])
            .with_title(locale.text("Chemistry editor")),
        ..Default::default()
    };
    restrict_backends(&mut options);
    let startup = locale.clone();
    eframe::run_native(
        "Chemistry editor",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::light());
            let mut app = Demo {
                editor: ChemEditor::sample_with_locale(startup),
                message: String::new(),
                frames: 0,
                shot: std::env::var("CHEM_SHOT").ok(),
            };
            if let Some(path) = std::env::args_os().nth(1) {
                if let Err(error) = app.load_path(std::path::Path::new(&path)) {
                    app.message = error;
                }
            }
            Ok(Box::new(app))
        }),
    )
}

/// Built-in language plus an optional flat JSON catalog from `CHEM_CATALOG`.
fn build_locale(code: &str) -> Locale {
    let base = Locale::new(code);
    let Ok(path) = std::env::var("CHEM_CATALOG") else {
        return base;
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return base;
    };
    base.with_catalog(&text)
        .unwrap_or_else(|_| Locale::new(code))
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
    editor: ChemEditor,
    message: String,
    frames: u32,
    shot: Option<String>,
}

impl Demo {
    fn open(&mut self) -> Result<(), String> {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(
                self.editor.locale().text("Chemistry JSON / DOCX"),
                &["json", "docx"],
            )
            .pick_file()
        else {
            return Ok(());
        };
        self.load_path(&path)?;
        if self.message.is_empty() {
            self.message = format!("Opened: {}", path.display());
        }
        Ok(())
    }

    fn load_path(&mut self, path: &std::path::Path) -> Result<(), String> {
        let mut bytes = vec![];
        std::fs::File::open(path)
            .map_err(|error| error.to_string())?
            .take(32 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("docx"))
        {
            #[cfg(feature = "docx")]
            {
                let imported = ChemDocument::from_docx(&bytes)?;
                self.editor.replace(imported.document)?;
                self.message = imported.issues.join("; ");
                return Ok(());
            }
            #[cfg(not(feature = "docx"))]
            return Err("DOCX support is not compiled in".into());
        }
        let json = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
        self.editor.replace(ChemDocument::from_json(json)?)?;
        Ok(())
    }

    fn save(&mut self, kind: &str) -> Result<(), String> {
        self.editor.commit();
        let Some(path) = rfd::FileDialog::new()
            .add_filter(kind, &[kind.to_ascii_lowercase().as_str()])
            .set_file_name(format!("chemistry.{}", kind.to_ascii_lowercase()))
            .save_file()
        else {
            return Ok(());
        };
        let content = match kind {
            "SVG" => self.editor.document().to_svg()?,
            _ => self.editor.document().to_json()?,
        };
        std::fs::write(&path, content).map_err(|error| error.to_string())?;
        self.message = format!("Saved: {}", path.display());
        Ok(())
    }

    fn save_png(&mut self) -> Result<(), String> {
        self.editor.commit();
        let bytes = self.editor.document().to_png(2.0)?;
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .set_file_name("chemistry.png")
            .save_file()
        else {
            return Ok(());
        };
        std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
        self.message = format!("Saved: {}", path.display());
        Ok(())
    }

    fn save_docx(&mut self) -> Result<(), String> {
        #[cfg(feature = "docx")]
        {
            self.editor.commit();
            let output = self.editor.document().to_docx()?;
            let Some(path) = rfd::FileDialog::new()
                .add_filter("DOCX", &["docx"])
                .set_file_name("chemistry.docx")
                .save_file()
            else {
                return Ok(());
            };
            std::fs::write(&path, output.bytes).map_err(|error| error.to_string())?;
            self.message = if output.issues.is_empty() {
                format!("Saved: {}", path.display())
            } else {
                output.issues.join("; ")
            };
            Ok(())
        }
        #[cfg(not(feature = "docx"))]
        Err("DOCX support is not compiled in".into())
    }
}

impl eframe::App for Demo {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let locale = self.editor.locale().clone();
        let t = |key: &str| locale.text(key).to_owned();
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(t("Language"));
                let mut code = self.editor.locale().code().to_owned();
                egui::ComboBox::from_id_salt("chem-language")
                    .selected_text(if code.starts_with("ru") {
                        "Русский"
                    } else {
                        "English"
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut code, "en".into(), "English");
                        ui.selectable_value(&mut code, "ru".into(), "Русский");
                    });
                if code != self.editor.locale().code() {
                    self.editor.set_locale(Locale::new(&code));
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title(
                        self.editor.locale().text("Chemistry editor").into(),
                    ));
                }
            });
            ui.horizontal(|ui| {
                ui.heading(
                    RichText::new(t("Structures and reactions"))
                        .color(Color32::from_rgb(43, 87, 154)),
                );
                ui.weak(t("EMBEDDABLE EGUI COMPONENT"));
                if ui.button(t("New document")).clicked() {
                    let _ = self.editor.replace(ChemDocument::default());
                }
                if ui.button(t("Open JSON")).clicked() {
                    if let Err(error) = self.open() {
                        self.message = error;
                    }
                }
                if ui.button(t("Save JSON")).clicked() {
                    if let Err(error) = self.save("JSON") {
                        self.message = error;
                    }
                }
                if ui.button(t("Export SVG")).clicked() {
                    if let Err(error) = self.save("SVG") {
                        self.message = error;
                    }
                }
                if ui.button(t("Export PNG")).clicked() {
                    if let Err(error) = self.save_png() {
                        self.message = error;
                    }
                }
                if ui.button(t("Export DOCX")).clicked() {
                    if let Err(error) = self.save_docx() {
                        self.message = error;
                    }
                }
            });
            ui.label(if self.message.is_empty() {
                t("Edit structures and reaction equations offline.")
            } else {
                locale.diagnostic(&self.message)
            });
            ui.separator();
            self.editor.show(ui);
        });
        self.shot(ui);
    }
}

impl Demo {
    fn shot(&mut self, ui: &mut egui::Ui) {
        let Some(path) = self.shot.clone() else {
            return;
        };
        self.frames += 1;
        if self.frames == 20 {
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
            let mut bytes = Vec::new();
            for pixel in &image.pixels {
                bytes.extend_from_slice(&pixel.to_array());
            }
            if let Err(error) = save_shot(&path, &bytes, image.size) {
                self.message = error;
            }
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ui.ctx().request_repaint();
    }
}

fn save_shot(path: &str, bytes: &[u8], size: [usize; 2]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|error| error.to_string())?;
    let width = u32::try_from(size[0]).map_err(|error| error.to_string())?;
    let height = u32::try_from(size[1]).map_err(|error| error.to_string())?;
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
    writer
        .write_image_data(bytes)
        .map_err(|error| error.to_string())
}
