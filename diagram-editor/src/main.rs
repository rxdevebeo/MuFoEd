//! Standalone review host; the library itself has no file dialogs or I/O.
use diagram_editor::{Diagram, DiagramEditor};
use egui::{Color32, RichText};
use std::io::Read as _;

fn main() -> eframe::Result {
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 780.0]),
        ..Default::default()
    };
    #[cfg(windows)]
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        use eframe::egui_wgpu::wgpu::Backends;
        setup.instance_descriptor.backends = Backends::from_env().unwrap_or(Backends::DX12);
    }
    eframe::run_native(
        "Редактор фигур и схем — MVP",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::light());
            let mut app = Demo {
                editor: {
                    let mut editor = DiagramEditor::sample();
                    if std::env::var_os("DIAGRAM_SELECT").is_some() {
                        let id = editor.diagram().nodes[0].id;
                        let _ = editor.select(Some(diagram_editor::Target::Node(id)));
                    }
                    editor
                },
                message: String::new(),
                frames: 0,
                shot: std::env::var("DIAGRAM_SHOT").ok(),
            };
            if let Some(path) = std::env::args_os().nth(1) {
                if let Err(error) = app.load_path(std::path::Path::new(&path)) {
                    app.message = error;
                }
            }
            if std::env::var_os("DIAGRAM_SELECT").is_some() {
                if let Some(id) = app.editor.diagram().nodes.first().map(|n| n.id) {
                    let _ = app.editor.select(Some(diagram_editor::Target::Node(id)));
                }
            }
            Ok(Box::new(app))
        }),
    )
}
struct Demo {
    editor: DiagramEditor,
    message: String,
    frames: u32,
    shot: Option<String>,
}
impl Demo {
    fn open(&mut self) -> Result<(), String> {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Схема JSON / DOCX", &["json", "docx"])
            .pick_file()
        else {
            return Ok(());
        };
        self.load_path(&path)?;
        if self.message.is_empty() {
            self.message = format!("Открыто: {}", path.display());
        }
        Ok(())
    }
    fn load_path(&mut self, path: &std::path::Path) -> Result<(), String> {
        let mut bytes = vec![];
        std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(32 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("docx"))
        {
            let imported = Diagram::from_docx(&bytes)?;
            self.editor.replace(imported.diagram)?;
            self.message = imported.issues.join("; ");
        } else {
            let json = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
            self.editor.replace(Diagram::from_json(json)?)?;
        }
        Ok(())
    }
    fn save_docx(&mut self) -> Result<(), String> {
        self.editor.commit();
        let output = self.editor.diagram().to_docx()?;
        let Some(path) = rfd::FileDialog::new()
            .add_filter("DOCX", &["docx"])
            .set_file_name("diagram.docx")
            .save_file()
        else {
            return Ok(());
        };
        std::fs::write(&path, output.bytes).map_err(|e| e.to_string())?;
        self.message = if output.issues.is_empty() {
            format!("Сохранено: {}", path.display())
        } else {
            output.issues.join("; ")
        };
        Ok(())
    }
    fn save(&mut self, svg: bool) -> Result<(), String> {
        self.editor.commit();
        let ext = if svg { "svg" } else { "json" };
        let Some(path) = rfd::FileDialog::new()
            .add_filter(ext, &[ext])
            .set_file_name(format!("diagram.{ext}"))
            .save_file()
        else {
            return Ok(());
        };
        let content = if svg {
            self.editor.diagram().to_svg()?
        } else {
            self.editor.diagram().to_json()?
        };
        std::fs::write(&path, content).map_err(|e| e.to_string())?;
        self.message = format!("Сохранено: {}", path.display());
        Ok(())
    }
}
impl eframe::App for Demo {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui,|ui|{
            ui.horizontal(|ui|{
                ui.heading(RichText::new("Фигуры и схемы").color(Color32::from_rgb(43,87,154)));
                ui.weak("MVP • отдельный egui-компонент");
                if ui.button("Открыть JSON / DOCX").clicked(){if let Err(e)=self.open(){self.message=e;}}
                if ui.button("Сохранить JSON").clicked(){if let Err(e)=self.save(false){self.message=e;}}
                if ui.button("Экспорт SVG").clicked(){if let Err(e)=self.save(true){self.message=e;}}
                if ui.button("Экспорт DOCX").clicked(){if let Err(e)=self.save_docx(){self.message=e;}}
                if ui.button("Новая схема").clicked(){let _=self.editor.replace(Diagram::default());}
            });
            ui.label(if self.message.is_empty(){"Схема редактируется автономно. JSON сохраняет фигуры и связи; SVG предназначен для просмотра."}else{&self.message});
            ui.separator();self.editor.show(ui);
        });
        if let Some(path) = &self.shot {
            self.frames += 1;
            if self.frames == 20 {
                ui.ctx()
                    .send_viewport_cmd(
                        egui::ViewportCommand::Screenshot(egui::UserData::default()),
                    );
            }
            let screenshot = ui.ctx().input(|i| {
                i.events.iter().find_map(|event| {
                    if let egui::Event::Screenshot { image, .. } = event {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(image) = screenshot {
                // The opt-in diagnostic host exports its own framebuffer through eframe.
                let mut bytes = Vec::new();
                for pixel in &image.pixels {
                    bytes.extend_from_slice(&pixel.to_array());
                }
                if let Err(e) = save_shot(path, &bytes, image.size) {
                    self.message = e;
                }
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ui.ctx().request_repaint();
        }
    }
}
fn save_shot(path: &str, bytes: &[u8], size: [usize; 2]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let width = u32::try_from(size[0]).map_err(|e| e.to_string())?;
    let height = u32::try_from(size[1]).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(bytes).map_err(|e| e.to_string())
}
