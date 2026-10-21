//! Embeddable egui editor. The host handles file dialogs through returned actions.
use crate::{
    generation::{Candidate, Mode, Request},
    Document, EditError, History, Raster,
};
use egui::{Color32, TextureHandle};
mod canvas_navigation;
mod clipboard_panel;
mod hires_panel;
mod layer_dialog;
mod preset_panel;
mod ribbon_ui;
mod selection_overlay;
mod transform_preview;
pub use preset_panel::GenerationPreset;
mod settings;
pub use settings::EditorSettings;
mod generation_panel;
mod geometry_dialog;
mod manual_panel;
mod runtime_panel;
pub use manual_panel::{EditTarget, ManualTool};
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

type RunningJob = crate::generation::PipelineJob;

/// File operation requested by the editor, to be handled by its host.
#[derive(Clone, Copy)]
pub enum FileAction {
    /// Import a raster as a new document.
    OpenImage,
    /// Import a raster as a new layer in the current document.
    ImportLayer,
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
    /// Import a named generation preset JSON library.
    ImportPresets,
    /// Export named generation presets to JSON.
    ExportPresets,
}

/// Reusable image editor widget with manual tools and generation staging.
#[allow(clippy::struct_excessive_bools)] // Independent rendering, persistence, dialog, and backend flags.
pub struct ImageEditor {
    runtime: mufoed_model_runtime::Runtime,
    document: Document,
    history: History,
    texture: Option<TextureHandle>,
    transform_preview: transform_preview::TransformPreview,
    candidate_texture: Option<TextureHandle>,
    changed: bool,
    unsaved: bool,
    confirm_new: bool,
    manual_ui: manual_panel::ManualUi,
    ribbon_ui: ribbon_ui::RibbonUi,
    selection_overlay: selection_overlay::SelectionOverlay,
    clipboard_ui: clipboard_panel::ClipboardUi,
    runtime_ui: runtime_panel::RuntimeUi,
    color: Color32,
    radius: f32,
    zoom: f32,
    navigation: canvas_navigation::Navigation,
    request: Request,
    executable: String,
    job: Option<RunningJob>,
    use_library: bool,
    candidate: Option<Candidate>,
    status: String,
    dimensions: [u32; 2],
    crop: [u32; 4],
    layer_dialog: layer_dialog::LayerDialog,
    pixel_grid: bool,
    blur: f32,
    generation_ui: generation_panel::GenerationUi,
    model_ui: model_panel::ModelUi,
    segmentation_ui: segmentation_panel::SegmentationUi,
    upscale_ui: upscale_panel::UpscaleUi,
    active_tab: StudioTab,
    presets: Vec<GenerationPreset>,
    preset_ui: preset_panel::PresetUi,
    settings_open: bool,
    settings_tab: usize,
    geometry: geometry_dialog::GeometryDialog,
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
        self.disable_hires_for_source_mode();
    }
    /// Choose generation from text, source pixels, or a painted mask.
    pub fn set_generation_mode(&mut self, mode: Mode) {
        self.request.mode = mode;
        self.disable_hires_for_source_mode();
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
            transform_preview: transform_preview::TransformPreview::default(),
            candidate_texture: None,
            changed: true,
            unsaved: false,
            confirm_new: false,
            manual_ui: manual_panel::ManualUi::default(),
            ribbon_ui: ribbon_ui::RibbonUi::default(),
            selection_overlay: selection_overlay::SelectionOverlay::default(),
            clipboard_ui: clipboard_panel::ClipboardUi::default(),
            runtime_ui: runtime_panel::RuntimeUi::default(),
            color: Color32::from_rgb(80, 160, 240),
            radius: 12.0,
            zoom: 1.0,
            navigation: canvas_navigation::Navigation::default(),
            request: Request {
                sampler: crate::generation::Sampler::Dpmpp2mKarras,
                postprocess: Some(crate::generation::Postprocess::default()),
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
            status: "Готово · рисуйте, откройте изображение или запустите генерацию".into(),
            dimensions,
            crop: [0, 0, dimensions[0], dimensions[1]],
            layer_dialog: layer_dialog::LayerDialog::default(),
            pixel_grid: false,
            blur: 1.5,
            generation_ui: generation_panel::GenerationUi::default(),
            model_ui: model_panel::ModelUi::default(),
            segmentation_ui: segmentation_panel::SegmentationUi::default(),
            upscale_ui: upscale_panel::UpscaleUi::default(),
            active_tab: StudioTab::Create,
            presets: Vec::new(),
            preset_ui: preset_panel::PresetUi::default(),
            settings_open: false,
            settings_tab: 0,
            geometry: geometry_dialog::GeometryDialog::default(),
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
        let ribbon_ui = std::mem::take(&mut self.ribbon_ui);
        let target = self.edit_target();
        let clipboard_ui = std::mem::take(&mut self.clipboard_ui);
        let executable = self.executable.clone();
        let request = self.request.clone();
        let use_library = self.use_library;
        let active_tab = self.active_tab;
        let pixel_grid = self.pixel_grid;
        let generation_ui = std::mem::take(&mut self.generation_ui);
        let runtime_ui = std::mem::take(&mut self.runtime_ui);
        let presets = std::mem::take(&mut self.presets);
        let mut model_ui = std::mem::take(&mut self.model_ui);
        model_ui.reset_project();
        let mut segmentation_ui = std::mem::take(&mut self.segmentation_ui);
        segmentation_ui.reset_project();
        let mut upscale_ui = std::mem::take(&mut self.upscale_ui);
        upscale_ui.reset_project();
        *self = Self::with_runtime(document, self.runtime.clone());
        self.set_edit_target(target);
        self.ribbon_ui = ribbon_ui;
        self.clipboard_ui = clipboard_ui;
        self.clipboard_ui.reset_document_position();
        self.executable = executable;
        self.request = request;
        self.use_library = use_library;
        self.active_tab = active_tab;
        self.pixel_grid = pixel_grid;
        self.generation_ui = generation_ui;
        self.runtime_ui = runtime_ui;
        self.presets = presets;
        self.model_ui = model_ui;
        self.segmentation_ui = segmentation_ui;
        self.upscale_ui = upscale_ui;
    }
    /// Show an actionable error or status in the editor.
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status = message.into();
    }
    fn edit(&mut self, operation: impl FnOnce(&mut Document) -> Result<(), EditError>) {
        self.finish_pasted_object();
        if self.layer_dialog_open() {
            self.layer_dialog = layer_dialog::LayerDialog::default();
            self.changed = true;
        }
        let before = self.document.clone();
        match operation(&mut self.document) {
            Ok(()) => {
                if self.document != before {
                    self.history.checkpoint(&before);
                    self.changed = true;
                    self.manual_ui.thumbnails.clear();
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
        if !ui.ctx().egui_wants_keyboard_input() {
            let mods = ui.input(|i| i.modifiers);
            if mods.command && ui.input(|i| i.key_pressed(egui::Key::S)) {
                actions.push(FileAction::SaveProject);
            }
            if mods.command && ui.input(|i| i.key_pressed(egui::Key::O)) {
                actions.push(FileAction::OpenImage);
            }
        }
        self.inline_transform_shortcuts(ui);
        self.manual_shortcuts(ui);
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
                    self.status = job.stage().into();
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
                ui.horizontal(|ui| {
                    let status_width = (ui.available_width() - 290.0).max(40.0);
                    ui.add_sized(
                        [status_width, 20.0],
                        egui::Label::new(&self.status).truncate(),
                    )
                    .on_hover_text(&self.status);
                    ui.label(format!(
                        "{}×{}",
                        self.document.width(),
                        self.document.height()
                    ));
                    let mut percent = self.zoom * 100.0;
                    if ui
                        .add(
                            egui::DragValue::new(&mut percent)
                                .range(2.0..=800.0)
                                .suffix("%"),
                        )
                        .changed()
                    {
                        self.zoom = percent / 100.0;
                    }
                    if ui.small_button("По размеру").clicked() {
                        self.manual_ui.fit_once = true;
                    }
                });
                let state = self.runtime.snapshot();
                if let Some(task) = state.active {
                    let phase = match state.phase {
                        Some(mufoed_model_runtime::Phase::Unknown) => {
                            "ожидание подтверждения выгрузки"
                        }
                        Some(mufoed_model_runtime::Phase::Releasing) => "освобождение модели",
                        Some(mufoed_model_runtime::Phase::Checking) => "проверка памяти",
                        Some(mufoed_model_runtime::Phase::Loading) => "загрузка",
                        Some(mufoed_model_runtime::Phase::Offloading) => "GPU → CPU",
                        Some(mufoed_model_runtime::Phase::Retrying) => "повтор после OOM",
                        Some(mufoed_model_runtime::Phase::Failed) => "ошибка",
                        Some(mufoed_model_runtime::Phase::Ready) => "готово",
                        _ => "выполнение",
                    };
                    ui.small(format!(
                        "Модели: {} · {} · {phase} · в очереди: {}",
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
        if !ui.ctx().egui_wants_keyboard_input()
            && ui.input_mut(|i| {
                i.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::Z,
                ))
            })
        {
            self.restore_history(true);
        }
        if !ui.ctx().egui_wants_keyboard_input()
            && ui.input_mut(|i| {
                i.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::COMMAND,
                    egui::Key::Z,
                ))
            })
        {
            self.restore_history(false);
        }
        self.ribbon(ui, &mut actions);
        egui::Panel::right("image-editing")
            .resizable(true)
            .default_size(310.0)
            .min_size(285.0)
            .frame(panel_frame())
            .show(ui, |ui| self.manual_dock(ui, &mut actions));
        if self.ribbon_ui.ai_visible {
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
                    } else if self.job.is_some() {
                        128.0
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
                                self.inpaint_prompt_panel(ui);
                                ui.separator();
                                self.model_tools_panel(ui, &mut actions);
                                self.segmentation_panel(ui, &mut actions);
                                ui.separator();
                                self.mask_actions(ui);
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
        }
        egui::Panel::left("manual-tool-strip")
            .resizable(false)
            .exact_size(50.0)
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .stroke(egui::Stroke::new(1.0, BORDER))
                    .inner_margin(6),
            )
            .show(ui, |ui| self.manual_palette(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_gray(30))
                    .inner_margin(14),
            )
            .show(ui, |ui| {
                if self.manual_ui.fit_once {
                    self.manual_ui.fit_once = false;
                    self.navigation = canvas_navigation::Navigation::default();
                    let available = ui.available_size();
                    self.zoom = ((available.x - 16.0) / self.document.width() as f32)
                        .min((available.y - 40.0) / self.document.height() as f32)
                        .clamp(0.02, 8.0);
                }
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Холст").strong().size(16.0));
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!(
                            "{} × {} · {} слоёв",
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
                    self.rebuild_selection_outline();
                    self.transform_preview.layers.clear();
                    let raster = self.document.flatten();
                    let mut pixels = raster.pixels().to_vec();
                    for (p, mask) in pixels.chunks_exact_mut(4).zip(self.document.mask()) {
                        if self.generation_ui.show_mask
                            && !self.mask_transform_preview()
                            && *mask > 0
                        {
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
                self.canvas_pan(ui);
                let offset = self.canvas_wheel_zoom(ui, canvas_space);
                let mut canvas_scroll =
                    egui::ScrollArea::both().id_salt(canvas_navigation::SCROLL_ID);
                if let Some(offset) = offset {
                    canvas_scroll = canvas_scroll.scroll_offset(offset);
                }
                canvas_scroll
                    .scroll_source(egui::scroll_area::ScrollSource {
                        drag: egui::scroll_area::DragScroll::Never,
                        ..Default::default()
                    })
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let size = self.preview_canvas_size() * self.zoom;
                        let (space, _) =
                            ui.allocate_exact_size(canvas_space.max(size), egui::Sense::hover());
                        let image_rect = egui::Rect::from_center_size(
                            space.center() + self.navigation.offset,
                            size,
                        );
                        let response = ui.interact(
                            image_rect,
                            ui.id().with("canvas-image"),
                            if self.manual_ui.tool == ManualTool::Hand && !self.has_pasted_object()
                            {
                                egui::Sense::hover()
                            } else {
                                egui::Sense::click_and_drag()
                            },
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
                        if !self.paint_transform_preview(ui, &painter, response.rect) {
                            painter.image(
                                self.texture.as_ref().expect("canvas texture").id(),
                                response.rect,
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                Color32::WHITE,
                            );
                        }
                        if !self.transform_preview_active()
                            && !self.navigation.interacting
                            && self.manual_ui.tool != ManualTool::Hand
                        {
                            self.segmentation_canvas(ui, &response);
                        }
                        if !self.navigation.interacting && !self.segmentation_point_mode() {
                            self.canvas_context_menu(&response);
                        }
                        self.layer_transform_handles(ui, &response);
                        self.paint_selection_outline(ui, response.rect);
                        if !self.transform_preview_active() {
                            self.draw_pixel_grid(ui, response.rect);
                            self.manual_canvas(ui, &response);
                        }
                    });
            });
        if self.confirm_new {
            egui::Window::new("Create new canvas?")
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
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
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
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
        self.runtime_panel(&context);
        actions.extend(self.settings_window(ui.ctx()));
        actions.extend(self.preset_window(ui.ctx()));
        self.geometry_window(ui.ctx());
        self.layer_window(ui.ctx());
        actions
    }
}
