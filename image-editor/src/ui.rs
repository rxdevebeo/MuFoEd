//! Embeddable egui editor. The host handles file dialogs through returned actions.
use crate::{
    generation::{Candidate, Job, Mode, Request},
    Adjustment, Document, EditError, History, Raster, Tool,
};
use egui::{Color32, TextureHandle};
mod generation_panel;
mod model_panel;
mod segmentation_panel;
mod upscale_panel;

/// Model workflow shown on the left; manual editing remains on the right.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StudioTab {
    /// Text-to-image or image-to-image generation.
    Create,
    /// Inpaint, background removal, and segmentation.
    Edit,
    /// Neural image enlargement and blending.
    Upscale,
}

// Match MuFoEd's blue accent and neutral dark ribbon palette (mufoed/src/theme.rs).
const ACCENT: Color32 = Color32::from_rgb(43, 87, 154);
const PANEL: Color32 = Color32::from_rgb(48, 48, 52);
const BORDER: Color32 = Color32::from_gray(70);

/// Apply the editor's dark workbench theme to a host context.
/// Embedding applications can opt in once during initialization.
pub fn apply_studio_style(context: &egui::Context) {
    let mut style = (*context.global_style()).clone();
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(18.0));
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.interact_size = egui::vec2(40.0, 32.0);
    style.spacing.window_margin = egui::Margin::same(14);
    style.spacing.combo_width = 200.0;
    style.visuals = egui::Visuals::dark();
    style.visuals.override_text_color = Some(Color32::from_gray(225));
    style.visuals.weak_text_color = Some(Color32::from_gray(160));
    style.visuals.panel_fill = PANEL;
    style.visuals.window_fill = PANEL;
    style.visuals.extreme_bg_color = Color32::from_gray(30);
    style.visuals.text_edit_bg_color = Some(Color32::from_rgb(38, 38, 42));
    style.visuals.faint_bg_color = Color32::from_rgb(58, 58, 63);
    style.visuals.window_stroke = egui::Stroke::new(1.0, BORDER);
    style.visuals.window_corner_radius = egui::CornerRadius::same(8);
    style.visuals.selection.bg_fill = ACCENT;
    style.visuals.selection.stroke = egui::Stroke::new(1.0, Color32::WHITE);
    style.visuals.hyperlink_color = Color32::from_rgb(120, 170, 240);
    for widget in [
        &mut style.visuals.widgets.noninteractive,
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::same(4);
        widget.bg_stroke = egui::Stroke::new(1.0, BORDER);
        widget.expansion = 0.0;
    }
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(58, 58, 63);
    style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(58, 58, 63);
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, Color32::from_gray(84));
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(70, 74, 82);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(70, 74, 82);
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT.gamma_multiply(0.7));
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(60, 88, 130);
    style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(60, 88, 130);
    style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, ACCENT);
    context.set_theme(egui::Theme::Dark);
    context.set_style_of(egui::Theme::Dark, style);
}

fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .inner_margin(14)
}

fn panel_header(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.heading(egui::RichText::new(title).strong());
    ui.label(egui::RichText::new(subtitle).small().weak());
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);
}

enum RunningJob {
    Process(Box<Job>),
    #[cfg(feature = "plakat")]
    Library(crate::generation::NativeJob),
}
impl RunningJob {
    fn start(
        library: bool,
        executable: &str,
        request: Request,
        doc: &Document,
        runtime: &mufoed_model_runtime::Runtime,
    ) -> Result<Self, EditError> {
        #[cfg(feature = "plakat")]
        if library {
            return crate::generation::NativeJob::start_with_runtime(request, doc, runtime)
                .map(Self::Library);
        }
        #[cfg(not(feature = "plakat"))]
        let _ = library;
        Job::start_with_runtime(executable, request, doc, runtime)
            .map(|j| Self::Process(Box::new(j)))
    }
    fn poll(&mut self) -> Result<Option<Candidate>, EditError> {
        match self {
            Self::Process(j) => j.poll(),
            #[cfg(feature = "plakat")]
            Self::Library(j) => j.poll(),
        }
    }
    fn cancel(&mut self) -> bool {
        match self {
            Self::Process(j) => {
                j.cancel();
                true
            }
            #[cfg(feature = "plakat")]
            Self::Library(j) => {
                j.cancel();
                false
            }
        }
    }
}

/// File operation requested by the editor, to be handled by its host.
#[derive(Clone, Copy)]
pub enum FileAction {
    /// Import a raster as a new document.
    OpenImage,
    /// Restore a layered project.
    OpenProject,
    /// Persist layers and mask.
    SaveProject,
    /// Export the flattened canvas as PNG.
    ExportPng,
    /// Add a local safetensors resource to the asset dialog.
    AddAsset,
    /// Choose local `BiRefNet` ONNX weights.
    BackgroundModel,
    /// Choose an existing Transformers SAM2 model directory.
    SegmentationModel,
    /// Choose local 4x-UltraSharp tensor weights.
    UltraSharpModel,
    /// Choose a local `AuraSR` v2 model/config directory.
    AuraSrModel,
}

/// Reusable image editor widget with manual tools and generation staging.
#[allow(clippy::struct_excessive_bools)] // Independent rendering, persistence, dialog, and backend flags.
pub struct ImageEditor {
    runtime: mufoed_model_runtime::Runtime,
    document: Document,
    history: History,
    texture: Option<TextureHandle>,
    candidate_texture: Option<TextureHandle>,
    changed: bool,
    unsaved: bool,
    confirm_new: bool,
    tool: Tool,
    color: Color32,
    radius: f32,
    zoom: f32,
    previous: Option<[f32; 2]>,
    request: Request,
    executable: String,
    job: Option<RunningJob>,
    use_library: bool,
    candidate: Option<Candidate>,
    status: String,
    dimensions: [u32; 2],
    crop: [u32; 4],
    brightness: i32,
    contrast: f32,
    blur: f32,
    generation_ui: generation_panel::GenerationUi,
    model_ui: model_panel::ModelUi,
    segmentation_ui: segmentation_panel::SegmentationUi,
    upscale_ui: upscale_panel::UpscaleUi,
    active_tab: StudioTab,
}
impl Default for ImageEditor {
    fn default() -> Self {
        Self::new(Document::new(768, 512).expect("default canvas"))
    }
}
impl ImageEditor {
    fn plakat_active(&self) -> bool {
        #[cfg(feature = "plakat")]
        if crate::generation::NativeJob::is_busy() {
            return true;
        }
        self.job.is_some()
    }
    /// Show a model workflow while retaining selected model settings.
    pub fn select_tab(&mut self, tab: StudioTab) {
        self.active_tab = tab;
        if tab != StudioTab::Edit {
            self.stop_segmentation_points();
        }
        if tab == StudioTab::Edit {
            self.request.mode = Mode::Inpaint;
        } else if tab == StudioTab::Create && self.request.mode == Mode::Inpaint {
            self.request.mode = Mode::TextToImage;
        }
    }
    /// Choose generation from text, source pixels, or a painted mask.
    pub fn set_generation_mode(&mut self, mode: Mode) {
        self.request.mode = mode;
        self.active_tab = if mode == Mode::Inpaint {
            StudioTab::Edit
        } else {
            StudioTab::Create
        };
    }
    /// Wrap a document in an editor with independent history.
    pub fn new(document: Document) -> Self {
        Self::with_runtime(document, mufoed_model_runtime::Runtime::global())
    }
    /// Embed with the same runtime instance used by the host's text/vision adapters.
    pub fn with_runtime(document: Document, runtime: mufoed_model_runtime::Runtime) -> Self {
        let dimensions = [document.width(), document.height()];
        let model = std::env::var("IMAGE_STUDIO_MODEL").unwrap_or_else(|_| "sd15".into());
        let family = if model.to_lowercase().contains("xl") {
            crate::assets::Family::Sdxl
        } else {
            crate::assets::Family::Sd15
        };
        Self {
            runtime,
            document,
            history: History::default(),
            texture: None,
            candidate_texture: None,
            changed: true,
            unsaved: false,
            confirm_new: false,
            tool: Tool::Brush,
            color: Color32::from_rgb(80, 160, 240),
            radius: 12.0,
            zoom: 1.0,
            previous: None,
            request: Request {
                model,
                family: Some(family),
                output_size: Some([family.size(); 2]),
                cache_dir: std::env::var_os("IMAGE_STUDIO_CACHE").map(Into::into),
                ..Request::default()
            },
            executable: std::env::var("IMAGE_STUDIO_PLAKAT").unwrap_or_else(|_| "plakat".into()),
            job: None,
            use_library: cfg!(feature = "plakat"),
            candidate: None,
            status: "Ready — paint, import a photo, or generate with Plakat".into(),
            dimensions,
            crop: [0, 0, dimensions[0], dimensions[1]],
            brightness: 10,
            contrast: 10.0,
            blur: 1.5,
            generation_ui: generation_panel::GenerationUi::default(),
            model_ui: model_panel::ModelUi::default(),
            segmentation_ui: segmentation_panel::SegmentationUi::default(),
            upscale_ui: upscale_panel::UpscaleUi::default(),
            active_tab: StudioTab::Create,
        }
    }
    /// Shared model coordinator, also used by the host's text and vision adapters.
    pub fn runtime(&self) -> &mufoed_model_runtime::Runtime {
        &self.runtime
    }
    /// Current project for host persistence and embedding.
    pub fn document(&self) -> &Document {
        &self.document
    }
    /// Whether an editing operation occurred after the most recent save/import.
    pub fn has_unsaved_changes(&self) -> bool {
        self.unsaved
    }
    /// Mark a successfully persisted project as saved. PNG export does not save layers.
    pub fn mark_saved(&mut self) {
        self.unsaved = false;
    }
    /// Replace the project and discard history/staged inference associated with the previous project.
    pub fn replace(&mut self, document: Document) {
        let executable = self.executable.clone();
        let request = self.request.clone();
        let use_library = self.use_library;
        let active_tab = self.active_tab;
        let generation_ui = std::mem::take(&mut self.generation_ui);
        let mut model_ui = std::mem::take(&mut self.model_ui);
        model_ui.reset_project();
        let mut segmentation_ui = std::mem::take(&mut self.segmentation_ui);
        segmentation_ui.reset_project();
        let mut upscale_ui = std::mem::take(&mut self.upscale_ui);
        upscale_ui.reset_project();
        *self = Self::with_runtime(document, self.runtime.clone());
        self.executable = executable;
        self.request = request;
        self.use_library = use_library;
        self.active_tab = active_tab;
        self.generation_ui = generation_ui;
        self.model_ui = model_ui;
        self.segmentation_ui = segmentation_ui;
        self.upscale_ui = upscale_ui;
    }
    /// Show an actionable error or status in the editor.
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status = message.into();
    }
    fn edit(&mut self, operation: impl FnOnce(&mut Document) -> Result<(), EditError>) {
        let before = self.document.clone();
        match operation(&mut self.document) {
            Ok(()) => {
                if self.document != before {
                    self.history.checkpoint(&before);
                    self.changed = true;
                    self.unsaved = true;
                    if self.document.layers() != before.layers() {
                        self.clear_segmentation_points();
                    }
                    if [self.document.width(), self.document.height()]
                        != [before.width(), before.height()]
                    {
                        self.dimensions = [self.document.width(), self.document.height()];
                        self.crop = [0, 0, self.document.width(), self.document.height()];
                    }
                }
            }
            Err(e) => {
                self.document = before;
                self.status = e.to_string();
            }
        }
    }
    /// Render the editor. Call each frame; generation polling never waits for inference.
    // Pixel/UI conversion intentionally rounds bounded canvas coordinates and color channels.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        clippy::too_many_lines
    )]
    pub fn show(&mut self, ui: &mut egui::Ui) -> Vec<FileAction> {
        let mut actions = Vec::new();
        let mut fit_canvas = false;
        let context = ui.ctx().clone();
        if self.job.is_none() && self.plakat_active() {
            self.status="Plakat завершает отменённую задачу; модели будут доступны после освобождения runtime".into();
            context.request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.poll_assets(&context);
        self.poll_model_tools(&context);
        self.poll_segmentation(&context);
        self.poll_upscale(&context);
        if let Some(job) = &mut self.job {
            match job.poll() {
                Ok(Some(candidate)) => {
                    self.candidate = Some(candidate);
                    self.candidate_texture = None;
                    self.job = None;
                    self.status = "Result ready — review and accept or discard".into();
                }
                Err(e) => {
                    self.status = e.to_string();
                    self.job = None;
                }
                Ok(None) => {
                    context.request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
        }
        egui::Panel::bottom("image-status")
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(36, 36, 40))
                    .inner_margin(egui::Margin::symmetric(14, 6)),
            )
            .show(ui, |ui| {
                ui.add(egui::Label::new(&self.status).truncate())
                    .on_hover_text(&self.status);
                let state = self.runtime.snapshot();
                if let Some(task) = state.active {
                    ui.small(format!(
                        "Модели: {} · {} · в очереди: {}",
                        task.owner,
                        task.model,
                        state.queued.len()
                    ));
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(100));
                } else if !state.queued.is_empty() {
                    ui.small(format!(
                        "Ожидают разрешения менеджера: {}",
                        state.queued.len()
                    ));
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(100));
                }
            });
        if ui.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::Z,
            ))
        }) {
            let changed = self.history.redo(&mut self.document);
            self.changed |= changed;
            self.unsaved |= changed;
        }
        if ui.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::Z,
            ))
        }) {
            let changed = self.history.undo(&mut self.document);
            self.changed |= changed;
            self.unsaved |= changed;
        }
        egui::Frame::new()
            .fill(PANEL)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("Image Studio").strong().size(16.0));
                    ui.separator();
                    for (label, action) in [
                        ("Open image", FileAction::OpenImage),
                        ("Open project", FileAction::OpenProject),
                        ("Save project", FileAction::SaveProject),
                        ("Export PNG", FileAction::ExportPng),
                    ] {
                        if ui.button(label).clicked() {
                            actions.push(action);
                        }
                    }
                    ui.separator();
                    if ui.button("Undo").clicked() {
                        let changed = self.history.undo(&mut self.document);
                        self.changed |= changed;
                        self.unsaved |= changed;
                    }
                    if ui.button("Redo").clicked() {
                        let changed = self.history.redo(&mut self.document);
                        self.changed |= changed;
                        self.unsaved |= changed;
                    }
                    ui.add(egui::Slider::new(&mut self.zoom, 0.1..=4.0).text("Zoom"));
                    if ui.button("Fit").clicked() {
                        fit_canvas = true;
                    }
                });
            });
        ui.separator();
        egui::Panel::right("image-editing")
            .resizable(true)
            .default_size(300.0)
            .min_size(270.0)
            .frame(panel_frame())
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("editing-scroll")
                    .show(ui, |ui| {
                        panel_header(ui, "Ручная работа", "Tools, adjustments and layers");
                        ui.label(egui::RichText::new("TOOLS").small().strong().weak());
                        let tool_width = (ui.available_width() - 8.0) * 0.5;
                        for row in [
                            [(Tool::Brush, "Brush"), (Tool::Eraser, "Eraser")],
                            [(Tool::Mask, "Mask"), (Tool::MaskEraser, "Unmask")],
                        ] {
                            ui.horizontal(|ui| {
                                for (tool, label) in row {
                                    if ui
                                        .add_sized(
                                            [tool_width, 36.0],
                                            egui::Button::new(label).selected(
                                                self.tool == tool
                                                    && !self.segmentation_point_mode(),
                                            ),
                                        )
                                        .clicked()
                                    {
                                        self.tool = tool;
                                        self.stop_segmentation_points();
                                    }
                                }
                            });
                        }
                        ui.horizontal(|ui| {
                            ui.label("Color");
                            ui.color_edit_button_srgba(&mut self.color);
                        });
                        ui.add(egui::Slider::new(&mut self.radius, 0.5..=128.0).text("Radius"));
                        ui.label(
                            egui::RichText::new("The red mask marks the area to regenerate.")
                                .small()
                                .weak(),
                        );
                        if ui.button("Clear mask").clicked() {
                            self.edit(|d| {
                                d.clear_mask();
                                Ok(())
                            });
                        }
                        ui.add_space(8.0);
                        ui.collapsing("Canvas", |ui| {
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::DragValue::new(&mut self.dimensions[0])
                                        .range(8..=8192)
                                        .prefix("W "),
                                );
                                ui.add(
                                    egui::DragValue::new(&mut self.dimensions[1])
                                        .range(8..=8192)
                                        .prefix("H "),
                                );
                            });
                            ui.horizontal(|ui| {
                                if ui.button("New").clicked() {
                                    self.confirm_new = true;
                                }
                                if ui.button("Resize").clicked() {
                                    let [w, h] = self.dimensions;
                                    self.edit(|d| d.resize(w, h));
                                }
                                if ui.button("Rotate 90°").clicked() {
                                    self.edit(|d| {
                                        d.rotate90();
                                        Ok(())
                                    });
                                }
                            });
                            ui.label("Crop: X / Y / width / height");
                            ui.horizontal_wrapped(|ui| {
                                for v in &mut self.crop {
                                    ui.add(egui::DragValue::new(v).range(0..=8192));
                                }
                            });
                            if ui.button("Apply crop").clicked() {
                                let [x, y, w, h] = self.crop;
                                self.edit(|d| d.crop(x, y, w, h));
                            }
                        });
                        ui.collapsing("Photo adjustments", |ui| {
                            ui.label(
                                egui::RichText::new("Applied to the selected layer")
                                    .small()
                                    .weak(),
                            );
                            ui.add(
                                egui::Slider::new(&mut self.brightness, -255..=255)
                                    .text("Brightness"),
                            );
                            if ui.button("Apply brightness").clicked() {
                                let v = self.brightness;
                                self.edit(|d| d.adjust(Adjustment::Brightness(v)));
                            }
                            ui.add(
                                egui::Slider::new(&mut self.contrast, -100.0..=100.0)
                                    .text("Contrast"),
                            );
                            if ui.button("Apply contrast").clicked() {
                                let v = self.contrast;
                                self.edit(|d| d.adjust(Adjustment::Contrast(v)));
                            }
                            ui.add(egui::Slider::new(&mut self.blur, 0.1..=20.0).text("Blur"));
                            if ui.button("Apply blur").clicked() {
                                let v = self.blur;
                                self.edit(|d| d.adjust(Adjustment::Blur(v)));
                            }
                            if ui.button("Grayscale").clicked() {
                                self.edit(|d| d.adjust(Adjustment::Grayscale));
                            }
                            if ui.button("Median denoise").clicked() {
                                self.edit(|d| d.adjust(Adjustment::Median));
                            }
                        });
                        ui.add_space(8.0);
                        ui.separator();
                        ui.separator();
                        ui.heading("Layers");
                        let mut selected = self.document.active();
                        for (index, layer) in self.document.layers().iter().enumerate().rev() {
                            if ui
                                .add_sized(
                                    [ui.available_width(), 36.0],
                                    egui::Button::new(layer.name()).selected(selected == index),
                                )
                                .clicked()
                            {
                                selected = index;
                            }
                        }
                        let _ = self.document.select(selected);
                        let layer = &self.document.layers()[selected];
                        let (mut name, mut visible, mut opacity) =
                            (layer.name().to_owned(), layer.visible(), layer.opacity());
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("Layer properties").small().weak());
                        let a = ui
                            .add_sized(
                                [ui.available_width(), 32.0],
                                egui::TextEdit::singleline(&mut name)
                                    .desired_width(f32::INFINITY)
                                    .margin(egui::Margin::symmetric(8, 7)),
                            )
                            .changed();
                        let b = ui.checkbox(&mut visible, "Visible").changed();
                        let c = ui
                            .add(egui::Slider::new(&mut opacity, 0..=255).text("Opacity"))
                            .changed();
                        if a || b || c {
                            self.edit(|d| {
                                d.set_layer(name, visible, opacity);
                                Ok(())
                            });
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("+ Layer").clicked() {
                                self.edit(|d| {
                                    d.add_layer(
                                        format!("Layer {}", d.layers().len() + 1),
                                        Raster::solid(d.width(), d.height(), [0; 4])?,
                                    )
                                });
                            }
                            if ui.button("Delete").clicked() {
                                self.edit(Document::remove_layer);
                            }
                            if ui.button("Up").clicked() {
                                self.edit(|d| {
                                    d.move_layer(true);
                                    Ok(())
                                });
                            }
                            if ui.button("Down").clicked() {
                                self.edit(|d| {
                                    d.move_layer(false);
                                    Ok(())
                                });
                            }
                        });
                    });
            });
        egui::Panel::left("image-generation")
            .resizable(true)
            .default_size(380.0)
            .min_size(340.0)
            .frame(panel_frame())
            .show(ui, |ui| {
                let previous = self.active_tab;
                ui.columns(3, |columns| {
                    for (column, (tab, label)) in columns.iter_mut().zip([
                        (StudioTab::Create, "Создание"),
                        (StudioTab::Edit, "Редактирование"),
                        (StudioTab::Upscale, "Апскейл"),
                    ]) {
                        column.style_mut().spacing.button_padding.x = 6.0;
                        if column
                            .add_sized(
                                [column.available_width(), 36.0],
                                egui::Button::new(egui::RichText::new(label).size(12.0))
                                    .selected(self.active_tab == tab),
                            )
                            .clicked()
                        {
                            self.active_tab = tab;
                        }
                    }
                });
                if previous != self.active_tab {
                    self.select_tab(self.active_tab);
                }
                ui.separator();
                let reserve = if self.active_tab == StudioTab::Upscale {
                    0.0
                } else {
                    84.0
                };
                egui::ScrollArea::vertical()
                    .id_salt(("model-panel", self.active_tab as u8))
                    .max_height((ui.available_height() - reserve).max(100.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.active_tab {
                        StudioTab::Create => {
                            self.generation_panel(ui, &mut actions);
                        }
                        StudioTab::Edit => {
                            panel_header(
                                ui,
                                "Редактирование с ИИ",
                                "Выделение, удаление фона и изменение по маске",
                            );
                            self.model_tools_panel(ui, &mut actions);
                            self.segmentation_panel(ui, &mut actions);
                            ui.separator();
                            self.generation_panel(ui, &mut actions);
                        }
                        StudioTab::Upscale => {
                            panel_header(
                                ui,
                                "Апскейл с ИИ",
                                "4× увеличение и смешивание результатов",
                            );
                            self.upscale_panel(ui, &mut actions);
                        }
                    });
                if self.active_tab != StudioTab::Upscale {
                    ui.separator();
                    self.generation_action(ui);
                }
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_gray(30))
                    .inner_margin(14),
            )
            .show(ui, |ui| {
                if fit_canvas {
                    let available = ui.available_size();
                    self.zoom = ((available.x - 16.0) / self.document.width() as f32)
                        .min((available.y - 40.0) / self.document.height() as f32)
                        .clamp(0.1, 4.0);
                }
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Canvas").strong().size(16.0));
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!(
                            "{} × {} · {} layers",
                            self.document.width(),
                            self.document.height(),
                            self.document.layers().len()
                        ))
                        .small()
                        .weak(),
                    );
                });
                ui.add_space(6.0);
                if self.changed || self.texture.is_none() {
                    let raster = self.document.flatten();
                    let mut pixels = raster.pixels().to_vec();
                    for (p, mask) in pixels.chunks_exact_mut(4).zip(self.document.mask()) {
                        if self.generation_ui.show_mask && *mask > 0 {
                            let f = f32::from(*mask) / 510.0;
                            p[0] = (f32::from(p[0]) * (1.0 - f) + 255.0 * f) as u8;
                            p[1] = (f32::from(p[1]) * (1.0 - f)) as u8;
                            p[2] = (f32::from(p[2]) * (1.0 - f)) as u8;
                            p[3] = p[3].max(100);
                        }
                    }
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [raster.width() as usize, raster.height() as usize],
                        &pixels,
                    );
                    if let Some(texture) = &mut self.texture {
                        texture.set(image, egui::TextureOptions::NEAREST);
                    } else {
                        self.texture = Some(context.load_texture(
                            "image-canvas",
                            image,
                            egui::TextureOptions::NEAREST,
                        ));
                    }
                    self.changed = false;
                }
                let canvas_space = ui.available_size();
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let size =
                            egui::vec2(self.document.width() as f32, self.document.height() as f32)
                                * self.zoom;
                        let (space, _) =
                            ui.allocate_exact_size(canvas_space.max(size), egui::Sense::hover());
                        let image_rect = egui::Rect::from_center_size(space.center(), size);
                        let response = ui.put(
                            image_rect,
                            egui::Image::new((
                                self.texture.as_ref().expect("canvas texture").id(),
                                size,
                            ))
                            .sense(egui::Sense::click_and_drag()),
                        );
                        // Checkerboard shows actual alpha; it is never written to the image.
                        let painter = ui
                            .painter()
                            .with_clip_rect(response.rect.intersect(ui.clip_rect()));
                        let cell = 16.0 * self.zoom;
                        let visible = response.rect.intersect(ui.clip_rect());
                        let start_x = ((visible.left() - response.rect.left()) / cell)
                            .floor()
                            .max(0.0) as usize;
                        let start_y = ((visible.top() - response.rect.top()) / cell)
                            .floor()
                            .max(0.0) as usize;
                        let end_x = ((visible.right() - response.rect.left()) / cell)
                            .ceil()
                            .max(0.0) as usize;
                        let end_y = ((visible.bottom() - response.rect.top()) / cell)
                            .ceil()
                            .max(0.0) as usize;
                        // Draw checkerboard then image over it, within the visible viewport only.
                        for y in start_y..end_y {
                            for x in start_x..end_x {
                                let rect = egui::Rect::from_min_size(
                                    response.rect.min
                                        + egui::vec2(x as f32 * cell, y as f32 * cell),
                                    egui::vec2(cell, cell),
                                );
                                painter.rect_filled(
                                    rect,
                                    0.0,
                                    if (x + y) % 2 == 0 {
                                        Color32::from_gray(35)
                                    } else {
                                        Color32::from_gray(43)
                                    },
                                );
                            }
                        }
                        painter.image(
                            self.texture.as_ref().expect("canvas texture").id(),
                            response.rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                        let down = ui.input(|i| i.pointer.primary_down());
                        self.segmentation_canvas(ui, &response);
                        if !self.segmentation_point_mode()
                            && (response.hovered() && down || response.dragged())
                        {
                            if let Some(position) = response
                                .interact_pointer_pos()
                                .or_else(|| ui.input(|i| i.pointer.hover_pos()))
                            {
                                let point = [
                                    (position.x - response.rect.left()) / self.zoom,
                                    (position.y - response.rect.top()) / self.zoom,
                                ];
                                if self.previous.is_none() {
                                    self.history.checkpoint(&self.document);
                                }
                                if let Err(e) = self.document.stroke(
                                    self.previous.unwrap_or(point),
                                    point,
                                    self.radius,
                                    self.color.to_srgba_unmultiplied(),
                                    self.tool,
                                ) {
                                    self.status = e.to_string();
                                }
                                self.previous = Some(point);
                                self.changed = true;
                                self.unsaved = true;
                                context.request_repaint();
                            }
                        } else if !down {
                            self.previous = None;
                        }
                    });
            });
        if self.confirm_new {
            egui::Window::new("Create new canvas?")
                .collapsible(false)
                .show(&context, |ui| {
                    ui.label(
                        "The current canvas will be replaced. Save the project first if needed.",
                    );
                    if ui.button("Create new canvas").clicked() {
                        match Document::new(self.dimensions[0], self.dimensions[1]) {
                            Ok(d) => self.replace(d),
                            Err(e) => self.status = e.to_string(),
                        }
                        self.confirm_new = false;
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_new = false;
                    }
                });
        }
        if let Some(candidate) = &self.candidate {
            if self.candidate_texture.is_none() {
                self.candidate_texture = Some(context.load_texture(
                    "generation-candidate",
                    egui::ColorImage::from_rgba_unmultiplied(
                        [
                            candidate.raster.width() as usize,
                            candidate.raster.height() as usize,
                        ],
                        candidate.raster.pixels(),
                    ),
                    egui::TextureOptions::LINEAR,
                ));
            }
            let mut accept = false;
            let mut discard = false;
            egui::Window::new("Generation result — preview")
                .default_width(520.0)
                .show(&context, |ui| {
                    ui.label(format!(
                        "{} · seed {}",
                        candidate.request.model, candidate.request.seed
                    ));
                    let texture = self.candidate_texture.as_ref().expect("candidate texture");
                    ui.add(egui::Image::new(texture).max_size(egui::vec2(500.0, 400.0)));
                    ui.horizontal(|ui| {
                        let different_size = [candidate.raster.width(), candidate.raster.height()]
                            != [self.document.width(), self.document.height()];
                        accept = ui
                            .button(if different_size {
                                "Accept as new image"
                            } else {
                                "Accept as new layer"
                            })
                            .clicked();
                        discard = ui.button("Discard").clicked();
                    });
                });
            if accept {
                let candidate = self.candidate.take().expect("candidate");
                self.edit(|d| {
                    if candidate.request.mode != Mode::TextToImage
                        && d.flatten() != candidate.source
                    {
                        return Err(EditError::Invalid(
                            "Canvas changed during generation; regenerate from the current canvas"
                                .into(),
                        ));
                    }
                    if candidate.request.mode == Mode::TextToImage
                        && [candidate.raster.width(), candidate.raster.height()]
                            != [d.width(), d.height()]
                    {
                        *d = Document::from_generated(candidate.raster, candidate.request)?;
                        return Ok(());
                    }
                    d.add_generated_layer(
                        format!(
                            "AI · {} · seed {}",
                            candidate.request.model, candidate.request.seed
                        ),
                        candidate.raster,
                        candidate.request,
                    )
                });
                self.candidate_texture = None;
            }
            if discard {
                self.candidate = None;
                self.candidate_texture = None;
            }
        }
        self.asset_dialog(&context, &mut actions);
        self.background_preview(&context);
        self.segmentation_preview(&context);
        self.upscale_preview(&context);
        actions
    }
}
