#[path = "huggingface_panel.rs"]
mod huggingface_panel;
use super::{panel_header, Color32, EditError, FileAction, ImageEditor, Mode, RunningJob, ACCENT};
use crate::{
    assets::{self, Asset, Family, Kind, Preparation},
    civitai::{self, Page, Resource},
    generation::Lora,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};

enum Event {
    Assets(Vec<Asset>),
    Page(Kind, Page),
    Downloaded(Asset),
    Progress(u64, Option<u64>),
    Error(String),
}
struct Worker {
    receiver: mpsc::Receiver<Event>,
    cancelled: Arc<AtomicBool>,
    label: String,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Catalog {
    Local,
    Civitai,
    HuggingFace,
}
pub(super) struct GenerationUi {
    assets: Vec<Asset>,
    pub(super) dialog: Option<Kind>,
    catalog: Catalog,
    hf: huggingface_panel::HuggingFaceUi,
    query: String,
    filter: String,
    root: String,
    base: String,
    token: String,
    resources: Vec<Resource>,
    cursor: Option<String>,
    worker: Option<Worker>,
    prepare: Option<Preparation>,
    progress: Option<(u64, Option<u64>)>,
    pub(super) model_name: String,
    custom_model: String,
    custom_family: Family,
    pub(super) random_seed: bool,
    pub(super) seed_text: String,
    pub(super) show_mask: bool,
}
impl Default for GenerationUi {
    fn default() -> Self {
        Self {
            assets: Vec::new(),
            dialog: None,
            catalog: Catalog::Local,
            hf: huggingface_panel::HuggingFaceUi::default(),
            query: String::new(),
            filter: String::new(),
            root: std::env::var("IMAGE_STUDIO_MODELS").unwrap_or_else(|_| "D:/Models".into()),
            base: "SDXL 1.0".into(),
            token: std::env::var("CIVITAI_TOKEN").unwrap_or_default(),
            resources: Vec::new(),
            cursor: None,
            worker: None,
            prepare: None,
            progress: None,
            model_name: String::new(),
            custom_model: String::new(),
            custom_family: Family::Sdxl,
            random_seed: true,
            seed_text: "42".into(),
            show_mask: true,
        }
    }
}
impl ImageEditor {
    /// Register a local resource selected by the embedding host's file dialog.
    pub fn add_asset(&mut self, path: &std::path::Path) -> Result<(), EditError> {
        let asset = assets::inspect(path)?;
        if let Some(kind) = self.generation_ui.dialog {
            if asset.kind != kind {
                return Err(EditError::Invalid(
                    "File does not match the open resource dialog".into(),
                ));
            }
        }
        if let Some(cache) = &self.request.cache_dir {
            assets::register(path, cache)?;
        }
        self.generation_ui.assets.retain(|a| a.path != asset.path);
        self.generation_ui.assets.push(asset);
        self.status = "Resource added to local library".into();
        Ok(())
    }
    fn scan_assets(&mut self) {
        if self.generation_ui.worker.is_some() {
            return;
        }
        let root = PathBuf::from(&self.generation_ui.root);
        let cache = self.request.cache_dir.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let event = match assets::scan(&root, cache.as_deref()) {
                Ok(a) => Event::Assets(a),
                Err(e) => Event::Error(e.to_string()),
            };
            let _ = sender.send(event);
        });
        self.generation_ui.worker = Some(Worker {
            receiver,
            cancelled: Arc::new(AtomicBool::new(false)),
            label: "Scanning local resources…".into(),
        });
    }
    /// Open a local model, adapter, or VAE selector in an embedding host.
    pub fn open_resource_dialog(&mut self, kind: Kind) {
        self.generation_ui.dialog = Some(kind);
        self.generation_ui.resources.clear();
        self.generation_ui.cursor = None;
        self.generation_ui.filter.clear();
        if kind == Kind::Vae {
            self.generation_ui.catalog = Catalog::Local;
        }
        if self.generation_ui.assets.is_empty() {
            self.scan_assets();
        }
    }
    /// Open the online checkpoint/adapter catalog. VAE selection stays local.
    pub fn open_civitai_dialog(&mut self, kind: Kind) {
        self.open_resource_dialog(kind);
        self.generation_ui.catalog = if kind == Kind::Vae {
            Catalog::Local
        } else {
            Catalog::Civitai
        };
    }
    pub(super) fn poll_assets(&mut self, context: &egui::Context) {
        self.poll_huggingface(context);
        let mut done = false;
        if let Some(worker) = &self.generation_ui.worker {
            loop {
                match worker.receiver.try_recv() {
                    Ok(event)
                        if worker.cancelled.load(Ordering::Acquire)
                            && !matches!(&event, Event::Progress(..)) =>
                    {
                        self.status = "Resource operation cancelled".into();
                        done = true;
                    }
                    Ok(Event::Progress(n, total)) => self.generation_ui.progress = Some((n, total)),
                    Ok(Event::Assets(found)) => {
                        self.generation_ui.assets.retain(|a| a.path.is_file());
                        for asset in found {
                            self.generation_ui.assets.retain(|a| a.path != asset.path);
                            self.generation_ui.assets.push(asset);
                        }
                        self.status =
                            format!("{} local resources", self.generation_ui.assets.len());
                        done = true;
                    }
                    Ok(Event::Page(kind, page)) => {
                        if self.generation_ui.dialog == Some(kind) {
                            self.generation_ui.resources = page.resources;
                            self.generation_ui.cursor = page.cursor;
                        }
                        self.status = "Civitai results received".into();
                        done = true;
                    }
                    Ok(Event::Downloaded(asset)) => {
                        self.generation_ui.assets.retain(|a| a.path != asset.path);
                        self.generation_ui.assets.push(asset);
                        self.generation_ui.catalog = Catalog::Local;
                        self.status = "SHA256 verified; resource added to local library".into();
                        done = true;
                    }
                    Ok(Event::Error(e)) => {
                        self.status = e;
                        done = true;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        if !done {
                            self.status = "Resource worker stopped".into();
                        }
                        done = true;
                        break;
                    }
                }
            }
        }
        if done {
            self.generation_ui.worker = None;
            self.generation_ui.progress = None;
        }
        if let Some(prepare) = &mut self.generation_ui.prepare {
            match prepare.poll() {
                Ok(Some((mut asset, result))) => {
                    self.generation_ui.prepare = None;
                    if asset.kind == Kind::Vae {
                        self.request.vae = Some(result.into());
                    } else {
                        asset.ready_model = Some(result.clone());
                        self.request.model = result;
                        self.request.family = Some(asset.family);
                        self.request.output_size = Some([asset.family.size(); 2]);
                        self.generation_ui.model_name.clone_from(&asset.name);
                    }
                    self.generation_ui.assets.retain(|a| a.path != asset.path);
                    self.generation_ui.assets.push(asset);
                    self.generation_ui.dialog = None;
                    self.status = "Resource prepared and selected".into();
                }
                Err(e) => {
                    self.generation_ui.prepare = None;
                    self.status = e.to_string();
                }
                Ok(None) => {}
            }
        }
        if self.generation_ui.worker.is_some()
            || self.generation_ui.prepare.is_some()
            || self.runtime_ui.pending.is_some()
        {
            context.request_repaint_after(Duration::from_millis(100));
        }
    }
    fn prepare_asset(&mut self, mut asset: Asset) {
        if asset.kind == Kind::Vae {
            asset.family = self.request.family.unwrap_or(Family::Sdxl);
        }
        let Some(cache) = &self.request.cache_dir else {
            self.status = "Set a local model cache before preparation".into();
            return;
        };
        match Preparation::start(asset, cache) {
            Ok(p) => self.generation_ui.prepare = Some(p),
            Err(e) => self.status = e.to_string(),
        }
    }
    fn search_civitai(&mut self, next: bool) {
        if self.generation_ui.worker.is_some() {
            return;
        }
        let kind = self.generation_ui.dialog.unwrap_or(Kind::Model);
        let query = self.generation_ui.query.clone();
        let base = self.generation_ui.base.clone();
        let token = self.generation_ui.token.clone();
        let cursor = if next {
            self.generation_ui.cursor.clone()
        } else {
            None
        };
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let event = match civitai::search(&query, kind, &base, cursor.as_deref(), &token) {
                Ok(p) => Event::Page(kind, p),
                Err(e) => Event::Error(e.to_string()),
            };
            let _ = sender.send(event);
        });
        self.generation_ui.worker = Some(Worker {
            receiver,
            cancelled: Arc::new(AtomicBool::new(false)),
            label: "Searching Civitai…".into(),
        });
    }
    fn download_resource(&mut self, resource: Resource) {
        let root = PathBuf::from(&self.generation_ui.root).join("Civitai");
        let token = self.generation_ui.token.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancelled.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let last = std::cell::Cell::new(Instant::now());
            let result = civitai::download(&resource, &root, &token, &worker_cancel, |n, total| {
                if last.get().elapsed() > Duration::from_millis(150) {
                    let _ = sender.send(Event::Progress(n, total));
                    last.set(Instant::now());
                }
            });
            let _ = sender.send(match result {
                Ok(a) => Event::Downloaded(a),
                Err(e) => Event::Error(e.to_string()),
            });
        });
        self.generation_ui.worker = Some(Worker {
            receiver,
            cancelled,
            label: "Downloading and verifying SHA256…".into(),
        });
    }
    fn choose_asset(&mut self, asset: Asset) {
        match asset.kind {
            Kind::Model => {
                if self.request.vae.is_none() {
                    self.request.vae = crate::huggingface::bundle_vae(&asset);
                }
                if let Some(model) = asset.ready_model {
                    self.request.model = model;
                    self.request.family = Some(asset.family);
                    self.request.output_size = Some([asset.family.size(); 2]);
                    self.generation_ui.model_name = asset.name;
                    self.generation_ui.dialog = None;
                } else {
                    self.prepare_asset(asset);
                }
            }
            Kind::Lora => {
                if !self.request.loras.iter().any(|l| l.path == asset.path) {
                    self.request.loras.push(Lora {
                        path: asset.path,
                        weight: 1.0,
                        enabled: true,
                    });
                }
                self.generation_ui.dialog = None;
            }
            Kind::Vae => self.prepare_asset(asset),
        }
    }
    fn new_seed(&mut self) -> bool {
        let mut bytes = [0; 8];
        match getrandom::fill(&mut bytes) {
            Ok(()) => {
                self.request.seed = u64::from_le_bytes(bytes);
                self.generation_ui.seed_text = self.request.seed.to_string();
                true
            }
            Err(e) => {
                self.status = format!("Seed generation failed: {e}");
                false
            }
        }
    }
    fn resolve_seed(&mut self) -> bool {
        if self.generation_ui.random_seed {
            return self.new_seed();
        }
        if let Ok(seed) = self.generation_ui.seed_text.parse::<u64>() {
            self.request.seed = seed;
            true
        } else {
            self.status = "Seed must be an integer in 0..18446744073709551615".into();
            false
        }
    }
    /// Whether the resource browser is still scanning, searching, preparing, or downloading.
    pub fn resource_operation_active(&self) -> bool {
        self.generation_ui.worker.is_some()
            || self.generation_ui.hf.busy()
            || self.generation_ui.prepare.is_some()
            || self.runtime_ui.pending.is_some()
    }
    #[allow(clippy::too_many_lines)] // Immediate-mode controls share editor state.
    pub(super) fn generation_panel(&mut self, ui: &mut egui::Ui, actions: &mut Vec<FileAction>) {
        if self.active_tab == super::StudioTab::Edit {
            self.request.mode = Mode::Inpaint;
            ui.strong("Inpaint · изменение выбранной области");
        } else {
            panel_header(
                ui,
                "Создание изображения",
                "Генерация по тексту или исходному изображению",
            );
            egui::ComboBox::from_id_salt("generation-mode")
                .width(ui.available_width())
                .selected_text(if self.request.mode == Mode::ImageToImage {
                    "По изображению · img2img"
                } else {
                    "По тексту · text to image"
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.request.mode, Mode::TextToImage, "По тексту");
                    ui.selectable_value(
                        &mut self.request.mode,
                        Mode::ImageToImage,
                        "По изображению",
                    );
                });
        }
        self.disable_hires_for_source_mode();
        if ui.button("Пресеты…").clicked() {
            self.open_presets();
        }
        self.source_controls(ui, actions);
        ui.label("Model");
        let label = if self.generation_ui.model_name.is_empty() {
            &self.request.model
        } else {
            &self.generation_ui.model_name
        };
        if ui
            .add_sized(
                [ui.available_width(), 36.0],
                egui::Button::new(format!("{label}  …")),
            )
            .clicked()
        {
            self.open_resource_dialog(Kind::Model);
        }
        ui.label(egui::RichText::new("Prompt").strong());
        ui.add(
            egui::TextEdit::multiline(&mut self.request.prompt)
                .desired_rows(4)
                .desired_width(f32::INFINITY)
                .margin(10)
                .hint_text("Describe the image…"),
        );
        ui.label("Negative prompt");
        ui.add(
            egui::TextEdit::multiline(&mut self.request.negative)
                .desired_rows(2)
                .desired_width(f32::INFINITY)
                .margin(10),
        );
        ui.separator();
        self.size_controls(ui);
        self.hires_ui(ui);
        ui.horizontal(|ui| {
            ui.label("Seed");
            ui.add(
                egui::TextEdit::singleline(&mut self.generation_ui.seed_text).desired_width(155.0),
            );
            if ui.button("Random").clicked() {
                self.new_seed();
            }
        });
        ui.checkbox(
            &mut self.generation_ui.random_seed,
            "New seed on each generation",
        );
        egui::Grid::new("generation-parameters")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Steps");
                ui.add(egui::DragValue::new(&mut self.request.steps).range(1..=150));
                ui.end_row();
                ui.label("Guidance");
                ui.add(
                    egui::DragValue::new(&mut self.request.guidance)
                        .range(0.0..=30.0)
                        .speed(0.1),
                );
                ui.end_row();
            });
        if self.request.mode == Mode::Inpaint {
            ui.separator();
            ui.label(egui::RichText::new("Mask").strong());
            self.mask_actions(ui);
            ui.checkbox(
                &mut self.request.mask_invert,
                "Regenerate outside painted area",
            );
            if ui
                .checkbox(&mut self.generation_ui.show_mask, "Show mask overlay")
                .changed()
            {
                self.show_inpaint_mask(self.generation_ui.show_mask);
            }
            ui.horizontal(|ui| {
                ui.label("Feather (px)");
                ui.add(egui::DragValue::new(&mut self.request.mask_feather).range(0..=128));
            });
            ui.small("Unmasked pixels are restored exactly after generation.");
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("LoRA").strong());
            if ui.button("Add…").clicked() {
                self.open_resource_dialog(Kind::Lora);
            }
        });
        let mut remove = None;
        for (index, lora) in self.request.loras.iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut lora.enabled, "");
                    ui.add(
                        egui::Label::new(
                            lora.path.file_stem().unwrap_or_default().to_string_lossy(),
                        )
                        .truncate(),
                    )
                    .on_hover_text(lora.path.display().to_string());
                    ui.add(
                        egui::DragValue::new(&mut lora.weight)
                            .range(-2.0..=2.0)
                            .speed(0.05),
                    );
                    if ui.small_button("×").clicked() {
                        remove = Some(index);
                    }
                });
            });
        }
        if let Some(i) = remove {
            self.request.loras.remove(i);
        }
        ui.horizontal(|ui| {
            ui.label("VAE");
            if ui.button("Choose…").clicked() {
                self.open_resource_dialog(Kind::Vae);
            }
            if self.request.vae.is_some() && ui.small_button("Default").clicked() {
                self.request.vae = None;
            }
        });
        ui.add(
            egui::Label::new(
                self.request
                    .vae
                    .as_ref()
                    .map_or_else(|| "Model default".into(), |p| p.display().to_string()),
            )
            .truncate(),
        );
        ui.add_space(6.0);
    }
    pub(super) fn generation_backend_settings(
        &mut self,
        ui: &mut egui::Ui,
        actions: &mut Vec<FileAction>,
    ) {
        egui::CollapsingHeader::new("Обработка после генерации")
            .default_open(
                self.request
                    .postprocess
                    .as_ref()
                    .is_some_and(|p| p.hires.is_some()),
            )
            .show(ui, |ui| {
                self.hires_ui(ui);
                if let Some(post) = &mut self.request.postprocess {
                    if post.hires.is_none() {
                        ui.add(egui::Slider::new(&mut post.denoise, 0.0..=0.5).text("Denoise"));
                        ui.horizontal(|ui| {
                            ui.label("Шаги Img2Img");
                            ui.add(egui::DragValue::new(&mut post.steps).range(1..=150));
                        });
                        ui.small(
                        "Img2Img → UltraSharp без увеличения, как в сохранённых старых настройках.",
                    );
                    }
                    ui.checkbox(&mut post.auto_device, "UltraSharp на GPU");
                    ui.add(egui::Label::new(post.ultrasharp.display().to_string()).truncate());
                    if ui.button("Выбрать 4x-UltraSharp…").clicked() {
                        actions.push(FileAction::UltraSharpModel);
                    }
                }
            });
        ui.collapsing("Engine settings", |ui| {
            #[cfg(feature = "plakat")]
            ui.add_enabled(
                self.job.is_none(),
                egui::Checkbox::new(&mut self.use_library, "Embedded Plakat library"),
            );
            if !self.use_library {
                ui.label("CLI executable");
                ui.text_edit_singleline(&mut self.executable);
            }
            ui.label("Cache (set IMAGE_STUDIO_CACHE before startup)");
            ui.label(self.request.cache_dir.as_ref().map_or_else(
                || "Default Hugging Face cache".into(),
                |p| p.display().to_string(),
            ));
        });
    }
    pub(super) fn inpaint_prompt_panel(&mut self, ui: &mut egui::Ui) {
        self.request.mode = Mode::Inpaint;
        ui.label(egui::RichText::new("Промпт перерисовки маски").strong());
        ui.add(
            egui::TextEdit::multiline(&mut self.request.prompt)
                .id(ui.make_persistent_id("inpaint-prompt"))
                .desired_rows(4)
                .desired_width(f32::INFINITY)
                .margin(10)
                .hint_text("Опишите, что должно появиться в выделенной области…"),
        );
        ui.collapsing("Негативный промпт", |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut self.request.negative)
                    .id_salt("inpaint-negative")
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Сила изменения");
            ui.add(egui::Slider::new(&mut self.request.strength, 0.0..=1.0));
        });
        ui.checkbox(&mut self.request.mask_invert, "Перерисовать вне маски");
        if ui
            .checkbox(
                &mut self.generation_ui.show_mask,
                "Показывать выделение / маску",
            )
            .changed()
        {
            self.show_inpaint_mask(self.generation_ui.show_mask);
        }
        ui.small("Модель и LoRA — в Настройках. Вне маски исходные пиксели сохраняются.");
    }
    pub(super) fn mask_actions(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Очистить").clicked() {
                self.edit(|d| {
                    d.clear_mask();
                    Ok(())
                });
            }
        });
    }
    pub(super) fn generation_action(&mut self, ui: &mut egui::Ui) {
        if self.job.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(self.job.as_ref().map_or("Генерация…", RunningJob::stage));
            });
            if self
                .runtime
                .snapshot()
                .active
                .is_some_and(|task| task.owner == "image")
            {
                if let Some(event) = self
                    .runtime
                    .events()
                    .last()
                    .filter(|event| !event.detail.is_empty())
                {
                    ui.small(&event.detail);
                }
            }
            if ui
                .add_sized(
                    [ui.available_width(), 40.0],
                    egui::Button::new("Cancel generation"),
                )
                .clicked()
            {
                if let Some(job) = &mut self.job {
                    job.cancel();
                }
                self.job = None;
                self.status = "Отмена цепочки; ожидаем завершения активного backend…".into();
            }
        } else if ui
            .add_enabled_ui(
                self.candidate.is_none()
                    && !self.plakat_active()
                    && self.generation_ui.prepare.is_none()
                    && !self.model_operation_active()
                    && !self.segmentation_operation_active()
                    && !self.upscale_operation_active(),
                |ui| {
                    ui.add_sized(
                        [ui.available_width(), 42.0],
                        egui::Button::new(
                            egui::RichText::new(if self.active_tab == super::StudioTab::Edit {
                                "Изменить по маске"
                            } else {
                                "Создать изображение"
                            })
                            .strong()
                            .color(Color32::WHITE),
                        )
                        .fill(ACCENT),
                    )
                },
            )
            .inner
            .clicked()
            && self.resolve_seed()
        {
            match RunningJob::start_with_runtime(
                self.use_library,
                &self.executable,
                self.request.clone(),
                &self.document,
                &self.runtime,
            ) {
                Ok(job) => {
                    self.job = Some(job);
                    self.status = format!("Generating · seed {}", self.request.seed);
                }
                Err(e) => self.status = e.to_string(),
            }
        }
    }
    pub(super) fn source_controls(&mut self, ui: &mut egui::Ui, actions: &mut Vec<FileAction>) {
        if self.request.mode != Mode::TextToImage {
            ui.label(format!(
                "Источник: текущий холст · {} × {}",
                self.document.width(),
                self.document.height()
            ));
            if ui
                .add_sized(
                    [ui.available_width(), 34.0],
                    egui::Button::new("Загрузить исходное изображение…"),
                )
                .clicked()
            {
                actions.push(FileAction::OpenImage);
            }
            ui.small("Изменяется текущий холст; результат сначала появится в предпросмотре.");
            ui.add(egui::Slider::new(&mut self.request.strength, 0.0..=1.0).text("Сила изменения"));
            ui.separator();
        }
    }
    fn size_controls(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Image size").strong());
        if self.request.mode != Mode::TextToImage {
            ui.label(format!(
                "{} × {} · source canvas",
                self.document.width(),
                self.document.height()
            ));
            return;
        }
        let family = self.request.family.unwrap_or_else(|| {
            if self.request.model.contains("xl") {
                Family::Sdxl
            } else {
                Family::Sd15
            }
        });
        let presets = if family == Family::Sdxl {
            [
                ("Model preset · 1:1", [1024, 1024]),
                ("4:3", [1152, 864]),
                ("3:4", [864, 1152]),
                ("16:9", [1280, 720]),
                ("9:16", [720, 1280]),
                ("3:2", [1152, 768]),
                ("2:3", [768, 1152]),
            ]
        } else {
            [
                ("Model preset · 1:1", [512, 512]),
                ("4:3", [640, 480]),
                ("3:4", [480, 640]),
                ("16:9", [768, 432]),
                ("9:16", [432, 768]),
                ("3:2", [576, 384]),
                ("2:3", [384, 576]),
            ]
        };
        let size = self
            .request
            .output_size
            .get_or_insert([self.document.width(), self.document.height()]);
        let selected = presets
            .iter()
            .find(|(_, s)| s == size)
            .map_or("Custom", |(n, _)| *n);
        egui::ComboBox::from_id_salt("size-preset")
            .selected_text(selected)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for (label, preset) in presets {
                    ui.selectable_value(size, preset, label);
                }
            });
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut size[0])
                    .range(64..=4096)
                    .speed(8.0)
                    .prefix("W "),
            );
            ui.label("×");
            ui.add(
                egui::DragValue::new(&mut size[1])
                    .range(64..=4096)
                    .speed(8.0)
                    .prefix("H "),
            );
            if ui.button("Swap").clicked() {
                size.swap(0, 1);
            }
        });
        ui.small("Multiples of 8; a different size opens as a new image after acceptance.");
    }
    #[allow(clippy::too_many_lines, clippy::cast_precision_loss)] // UI ratios and MiB labels are approximate.
    pub(super) fn asset_dialog(&mut self, context: &egui::Context, actions: &mut Vec<FileAction>) {
        let Some(kind) = self.generation_ui.dialog else {
            return;
        };
        let mut open = true;
        let title = match kind {
            Kind::Model => "Select model",
            Kind::Lora => "Add LoRA",
            Kind::Vae => "Select optional VAE",
        };
        egui::Window::new(title).id(egui::Id::new("resource-dialog")).open(&mut open).anchor(egui::Align2::CENTER_CENTER,egui::Vec2::ZERO).default_size([760.0,560.0]).min_width(600.0).max_width(760.0).resizable([false,true]).min_height(480.0).collapsible(false).show(context,|ui|{
            ui.horizontal(|ui|{ui.selectable_value(&mut self.generation_ui.catalog,Catalog::Local,"Local library");if kind!=Kind::Vae {ui.selectable_value(&mut self.generation_ui.catalog,Catalog::Civitai,"Civitai");}ui.selectable_value(&mut self.generation_ui.catalog,Catalog::HuggingFace,"Hugging Face");});ui.separator();
            let busy=self.generation_ui.worker.is_some() || self.generation_ui.hf.busy() || self.generation_ui.prepare.is_some() || self.runtime_ui.pending.is_some();
            if let Some(worker)=&self.generation_ui.worker {
                ui.horizontal(|ui|{ui.spinner();ui.label(&worker.label);if ui.button("Cancel").clicked(){worker.cancelled.store(true,Ordering::Release);}});
                if let Some((n,total))=self.generation_ui.progress {
                    ui.label(format!("{:.1} MiB{}",n as f64/1_048_576.0,total.map_or_else(String::new,|t|format!(" / {:.1} MiB",t as f64/1_048_576.0))));
                    if let Some(t)=total.filter(|t|*t>0) {ui.add(egui::ProgressBar::new(n as f32/t as f32));}
                }
            }
            if self.generation_ui.prepare.is_some() {ui.horizontal(|ui|{ui.spinner();ui.label("Preparing weights locally…");if ui.button("Cancel preparation").clicked(){self.generation_ui.prepare=None;}});}
            if self.generation_ui.catalog == Catalog::HuggingFace {
                self.huggingface_panel(ui,kind,busy);
            } else if self.generation_ui.catalog == Catalog::Civitai {
                ui.horizontal(|ui|{ui.add(egui::TextEdit::singleline(&mut self.generation_ui.query).hint_text("Search model or LoRA").desired_width(330.0));
                    egui::ComboBox::from_id_salt("civitai-base").selected_text(if self.generation_ui.base.is_empty(){"All families"}else{&self.generation_ui.base}).show_ui(ui,|ui|{for name in ["SDXL 1.0","SD 1.5","Illustrious","Pony",""] {ui.selectable_value(&mut self.generation_ui.base,name.to_owned(),if name.is_empty(){"All families"}else{name});}});
                    if ui.add_enabled(!busy,egui::Button::new("Search")).clicked(){self.search_civitai(false);}
                });
                ui.collapsing("Civitai access",|ui|{ui.label("Optional API token (memory only, or CIVITAI_TOKEN environment variable)");ui.add(egui::TextEdit::singleline(&mut self.generation_ui.token).password(true).desired_width(f32::INFINITY));ui.hyperlink_to("Account settings","https://civitai.com/user/account");});
                ui.small("Select a version and file. Check the author's license on the model page. Downloaded checkpoints require local preparation.");
                let mut download=None;
                egui::ScrollArea::vertical().id_salt("civitai-results").auto_shrink([false,false]).max_height(350.0).show(ui,|ui|{
                    let row_width=(ui.available_width()-egui::Frame::group(ui.style()).total_margin().sum().x).max(0.0);
                    for resource in &self.generation_ui.resources {ui.group(|ui|{ui.set_width(row_width);
                        ui.label(egui::RichText::new(format!("{} · {}",resource.name,resource.version)).strong());
                        ui.label(format!("{} · {} · {:.0} MiB",resource.base_model,resource.file_name,resource.size_kb/1024.0));
                        if !resource.trained_words.is_empty(){ui.small(format!("Trigger words: {}",resource.trained_words.join(", ")));}
                        let family=Family::parse(&resource.base_model);
                        let compatible=family!=Family::Unknown && (kind!=Kind::Lora || self.request.family.is_none_or(|f|f==family || f==Family::Unknown));
                        ui.horizontal(|ui|{ui.hyperlink_to("Model / license",format!("https://civitai.com/models/{}?modelVersionId={}",resource.model_id,resource.version_id));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add_enabled(!busy && compatible,egui::Button::new(egui::RichText::new("Download").color(Color32::WHITE)).fill(ACCENT).min_size(egui::vec2(160.0,32.0))).clicked(){download=Some(resource.clone());}
                                if !compatible {ui.label("Unsupported or incompatible family");}
                            });
                        });
                    });}
                });
                if let Some(resource)=download {self.download_resource(resource);}
                if ui.add_enabled(!busy && self.generation_ui.cursor.is_some(),egui::Button::new("Next page")).clicked(){self.search_civitai(true);}
            } else {
                ui.horizontal(|ui|{ui.label("Folder");ui.text_edit_singleline(&mut self.generation_ui.root);if ui.add_enabled(!busy,egui::Button::new("Refresh")).clicked(){self.scan_assets();}
if ui.add_enabled(!busy,egui::Button::new("Add file…")).clicked(){actions.push(FileAction::AddAsset);}});
                ui.add(egui::TextEdit::singleline(&mut self.generation_ui.filter).hint_text("Filter local library").desired_width(f32::INFINITY));
                let filter=self.generation_ui.filter.to_lowercase();let mut chosen=None;
                egui::ScrollArea::vertical().id_salt("local-resources").auto_shrink([false,false]).max_height(330.0).show(ui,|ui|{
                    let row_width=(ui.available_width()-egui::Frame::group(ui.style()).total_margin().sum().x).max(0.0);
                    for asset in self.generation_ui.assets.iter().filter(|a|a.kind==kind && a.name.to_lowercase().contains(&filter)) {ui.group(|ui|{ui.set_width(row_width);
                        ui.label(egui::RichText::new(&asset.name).strong());ui.add(egui::Label::new(asset.path.display().to_string()).truncate());
                        let compatible=kind!=Kind::Lora || self.request.family.is_none_or(|f|f==Family::Unknown || asset.family==Family::Unknown || f==asset.family);
                        ui.horizontal(|ui|{ui.label(format!("{:?} · {}",asset.family,if asset.ready_model.is_some() || kind==Kind::Lora {"Ready"}else{"Needs preparation"}));
                            let supported=kind!=Kind::Model || asset.family!=Family::Unknown;
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let label=if asset.ready_model.is_some() || kind==Kind::Lora {"Select"} else {"Prepare & select"};
                                if ui.add_enabled(!busy && supported && compatible,egui::Button::new(egui::RichText::new(label).color(Color32::WHITE)).fill(ACCENT).min_size(egui::vec2(160.0,32.0))).clicked(){chosen=Some(asset.clone());}
                            });
                        });
                        if kind==Kind::Lora && asset.family==Family::Unknown {ui.small("Family unknown: verify compatibility before generation.");}
                    });}
                });
                if let Some(asset)=chosen {self.choose_asset(asset);}
                if kind==Kind::Model {ui.collapsing("Plakat alias / prepared Hugging Face ID",|ui|{
                    ui.text_edit_singleline(&mut self.generation_ui.custom_model);
                    egui::ComboBox::from_id_salt("custom-family").selected_text(format!("{:?}",self.generation_ui.custom_family)).show_ui(ui,|ui|{ui.selectable_value(&mut self.generation_ui.custom_family,Family::Sdxl,"SDXL");ui.selectable_value(&mut self.generation_ui.custom_family,Family::Sd15,"SD 1.5");});
                    if ui.add_enabled(!busy && !self.generation_ui.custom_model.trim().is_empty(),egui::Button::new("Use identifier")).clicked(){self.generation_ui.custom_model.trim().clone_into(&mut self.request.model);self.request.family=Some(self.generation_ui.custom_family);self.request.output_size=Some([self.generation_ui.custom_family.size();2]);self.generation_ui.model_name.clear();self.generation_ui.dialog=None;}
                });}
            }
            ui.separator();ui.add(egui::Label::new(&self.status).wrap());
        });
        if !open {
            self.generation_ui.dialog = None;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn edit_prompt_is_visible_and_typing_updates_the_inpaint_request() {
        let ctx = egui::Context::default();
        let mut editor = super::ImageEditor::default();
        editor.select_tab(crate::ui::StudioTab::Edit);
        let mut point = egui::pos2(80.0, 80.0);
        for stage in 0..4 {
            let events = match stage {
                1 | 2 => vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed: stage == 1,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                3 => vec![egui::Event::Text("a small red vase".into())],
                _ => vec![],
            };
            let mut id = egui::Id::NULL;
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(380.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        id = ui.make_persistent_id("inpaint-prompt");
                        editor.inpaint_prompt_panel(ui);
                    });
                },
            );
            assert!(!output.shapes.is_empty());
            point = ctx
                .read_response(id)
                .expect("visible prompt editor")
                .rect
                .center();
        }
        assert_eq!(editor.request.prompt, "a small red vase");
        assert_eq!(editor.request.mode, crate::generation::Mode::Inpaint);
    }

    use super::*;
    #[test]
    fn seed_text_retains_full_u64_without_float_rounding() {
        let mut editor = ImageEditor::default();
        editor.generation_ui.random_seed = false;
        editor.generation_ui.seed_text = u64::MAX.to_string();
        assert!(editor.resolve_seed());
        assert_eq!(editor.request.seed, u64::MAX);
        editor.generation_ui.seed_text = "18446744073709551616".into();
        assert!(!editor.resolve_seed());
        assert_eq!(editor.request.seed, u64::MAX);
        assert!(editor.new_seed());
        assert_eq!(
            editor.generation_ui.seed_text,
            editor.request.seed.to_string()
        );
    }
}
