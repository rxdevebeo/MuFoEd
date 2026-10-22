//! Hugging Face catalog in the existing model/LoRA/VAE selector.
#![allow(clippy::cast_precision_loss)] // Approximate transfer sizes and progress ratios.
use super::{Asset, Catalog, Color32, EditError, Family, ImageEditor, Kind, ACCENT};
use crate::huggingface::{self, Model, Repository};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
enum Event {
    Page(huggingface::Page),
    Repository(Repository),
    Downloaded(Asset),
    Progress(u64, Option<u64>),
    Error(String),
}
struct HfWorker {
    receiver: mpsc::Receiver<Event>,
    cancelled: Arc<AtomicBool>,
    label: String,
}
impl Drop for HfWorker {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}
pub(super) struct HuggingFaceUi {
    query: String,
    repo: String,
    revision: String,
    token: String,
    models: Vec<Model>,
    next: Option<String>,
    repository: Option<Repository>,
    family: Family,
    worker: Option<HfWorker>,
    progress: Option<(u64, Option<u64>)>,
}
impl Default for HuggingFaceUi {
    fn default() -> Self {
        Self {
            query: String::new(),
            repo: String::new(),
            revision: "main".into(),
            token: std::env::var("HF_TOKEN").unwrap_or_default(),
            models: Vec::new(),
            next: None,
            repository: None,
            family: Family::Unknown,
            worker: None,
            progress: None,
        }
    }
}
impl HuggingFaceUi {
    pub(super) fn cancel(&mut self) {
        self.worker = None;
        self.progress = None;
    }
    pub(super) fn busy(&self) -> bool {
        self.worker.is_some()
    }
}
impl ImageEditor {
    /// Open Hugging Face search and pinned repository import for the selected resource role.
    pub fn open_huggingface_dialog(&mut self, kind: Kind) {
        self.open_resource_dialog(kind);
        self.generation_ui.catalog = Catalog::HuggingFace;
    }
    /// Open a repository's file list asynchronously at a chosen branch, tag or commit.
    pub fn open_huggingface_repository(&mut self, repo: &str, revision: &str, kind: Kind) {
        self.open_huggingface_dialog(kind);
        self.generation_ui.hf.repo = repo.into();
        self.generation_ui.hf.revision = revision.into();
        self.hf_open(None);
    }
    pub(super) fn poll_huggingface(&mut self, ctx: &egui::Context) {
        let hf = &mut self.generation_ui.hf;
        let mut done = false;
        if let Some(worker) = &hf.worker {
            loop {
                match worker.receiver.try_recv() {
                    Ok(_) if worker.cancelled.load(Ordering::Acquire) => {
                        self.status = "Hugging Face: операция отменена".into();
                        done = true;
                    }
                    Ok(Event::Page(page)) => {
                        hf.models = page.models;
                        hf.next = page.next;
                        hf.repository = None;
                        self.status = "Hugging Face: результаты поиска получены".into();
                        done = true;
                    }
                    Ok(Event::Repository(repo)) => {
                        hf.family = repo.family();
                        hf.repo.clone_from(&repo.id);
                        hf.repository = Some(repo);
                        self.status = "Hugging Face: файлы выбранной ревизии получены".into();
                        done = true;
                    }
                    Ok(Event::Downloaded(asset)) => {
                        self.generation_ui.assets.retain(|a| a.path != asset.path);
                        self.generation_ui.assets.push(asset);
                        self.generation_ui.catalog = Catalog::Local;
                        self.status="Hugging Face: загрузка проверена, ресурс добавлен в локальную библиотеку".into();
                        done = true;
                    }
                    Ok(Event::Progress(n, total)) => hf.progress = Some((n, total)),
                    Ok(Event::Error(e)) => {
                        self.status = e;
                        done = true;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        if !done {
                            self.status =
                                "Hugging Face: фоновая операция завершилась без результата".into();
                        }
                        done = true;
                        break;
                    }
                }
            }
        }
        if done {
            hf.worker = None;
            hf.progress = None;
            ctx.request_repaint();
        } else if hf.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    fn hf_search(&mut self, next: bool) {
        if self.generation_ui.hf.busy() {
            return;
        }
        let hf = &self.generation_ui.hf;
        let query = hf.query.clone();
        let token = hf.token.clone();
        let cursor = if next { hf.next.clone() } else { None };
        self.hf_task("Поиск Hugging Face…", move |_, _| {
            huggingface::search(&query, cursor.as_deref(), &token).map(Event::Page)
        });
    }
    fn hf_open(&mut self, repo: Option<String>) {
        if self.generation_ui.hf.busy() {
            return;
        }
        if let Some(repo) = repo {
            self.generation_ui.hf.repo = repo;
        }
        let hf = &self.generation_ui.hf;
        let repo = hf.repo.clone();
        let revision = hf.revision.clone();
        let token = hf.token.clone();
        self.hf_task(
            "Получение файлов Hugging Face…",
            move |_, _| huggingface::repository(&repo, &revision, &token).map(Event::Repository),
        );
    }
    fn hf_task(
        &mut self,
        label: &str,
        task: impl FnOnce(&AtomicBool, &mpsc::Sender<Event>) -> Result<Event, EditError>
            + Send
            + 'static,
    ) {
        let (sender, receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        std::thread::spawn(move || {
            let event = task(&flag, &sender).unwrap_or_else(|e| Event::Error(e.to_string()));
            let _ = sender.send(event);
        });
        self.generation_ui.hf.progress = None;
        self.generation_ui.hf.worker = Some(HfWorker {
            receiver,
            cancelled,
            label: label.into(),
        });
        self.status = label.into();
    }
    fn hf_download(&mut self, file: Option<huggingface::File>, kind: Kind) {
        if self.generation_ui.hf.busy() {
            return;
        }
        let Some(repo) = self.generation_ui.hf.repository.clone() else {
            return;
        };
        let family = self.generation_ui.hf.family;
        let token = self.generation_ui.hf.token.clone();
        let root = PathBuf::from(&self.generation_ui.root).join("HuggingFace");
        let cache = self.request.cache_dir.clone();
        self.hf_task(
            "Загрузка и проверка Hugging Face…",
            move |cancel, sender| {
                let last = std::cell::Cell::new(Instant::now());
                let progress = |n, total| {
                    if last.get().elapsed() > Duration::from_millis(150) {
                        let _ = sender.send(Event::Progress(n, total));
                        last.set(Instant::now());
                    }
                };
                let asset = if let Some(file) = file {
                    huggingface::download(
                        &repo,
                        &file,
                        (kind, family),
                        &root,
                        &token,
                        cancel,
                        progress,
                    )?
                } else {
                    huggingface::download_diffusers(
                        &repo,
                        family,
                        cache.as_deref().ok_or_else(|| {
                            EditError::Invalid("Задайте кэш моделей в настройках".into())
                        })?,
                        &token,
                        cancel,
                        progress,
                    )?
                };
                if let Some(cache) = cache {
                    if asset.ready_model.is_none() {
                        crate::assets::register(&asset.path, &cache)?;
                    }
                }
                Ok(Event::Downloaded(asset))
            },
        );
    }
    pub(super) fn huggingface_panel(&mut self, ui: &mut egui::Ui, kind: Kind, busy: bool) {
        self.hf_progress(ui);
        let mut search = false;
        let mut open = false;
        let mut next = false;
        let mut chosen = None;
        let mut download = None;
        let mut bundle = false;
        ui.add_enabled_ui(!busy, |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.generation_ui.hf.query)
                        .hint_text("Поиск Hugging Face")
                        .desired_width(460.0),
                );
                search = ui.button("Поиск").clicked();
            });
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.generation_ui.hf.repo)
                        .hint_text("owner/repo или ссылка на репозиторий")
                        .desired_width(360.0),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.generation_ui.hf.revision)
                        .hint_text("Revision")
                        .desired_width(100.0),
                );
                open = ui.button("Открыть").clicked();
            });
            ui.collapsing("Доступ Hugging Face", |ui| {
                ui.small("HF_TOKEN или read-токен — только в памяти, без сохранения в настройках");
                ui.add(
                    egui::TextEdit::singleline(&mut self.generation_ui.hf.token)
                        .password(true)
                        .desired_width(f32::INFINITY),
                );
                ui.hyperlink_to("Токены доступа", "https://huggingface.co/settings/tokens");
            });
        });
        if self.generation_ui.hf.repository.is_some() {
            (download, bundle) = self.hf_repository_ui(ui, kind, busy);
        } else {
            (chosen, next) = self.hf_results_ui(ui, busy);
        }
        if search || next {
            self.hf_search(next);
        } else if open || chosen.is_some() {
            self.hf_open(chosen);
        } else if bundle || download.is_some() {
            self.hf_download(download, kind);
        }
    }
    fn hf_repository_ui(
        &mut self,
        ui: &mut egui::Ui,
        kind: Kind,
        busy: bool,
    ) -> (Option<huggingface::File>, bool) {
        let mut download = None;
        let mut bundle = false;
        let repo = self
            .generation_ui
            .hf
            .repository
            .as_ref()
            .expect("repository view");
        ui.horizontal_wrapped(|ui| {
            ui.hyperlink_to(&repo.id, format!("https://huggingface.co/{}", repo.id));
            ui.label(format!("{} · {}", &repo.sha[..8], repo.license()));
            if repo.gated != serde_json::Value::Bool(false) && !repo.gated.is_null() {
                ui.weak("Требуется доступ автора");
            }
        });
        ui.horizontal(|ui| {
            ui.label("Архитектура");
            ui.add_enabled_ui(!busy, |ui| {
                for (f, name) in [
                    (Family::Unknown, "Не определена"),
                    (Family::Sd15, "SD 1.5"),
                    (Family::Sdxl, "SDXL"),
                ] {
                    ui.selectable_value(&mut self.generation_ui.hf.family, f, name);
                }
            });
        });
        if kind == Kind::Model {
            match huggingface::diffusers_plan(repo, self.generation_ui.hf.family) {
                Ok(plan) => {
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "Diffusers · {:.1} GiB",
                            plan.size as f64 / 1_073_741_824.0
                        ));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            bundle = primary(ui, !busy, "Загрузить Diffusers").clicked();
                        });
                    });
                }
                Err(_) => {
                    ui.small("Для Diffusers нужны стандартные компоненты SD 1.5/SDXL; либо выберите полный checkpoint ниже.");
                }
            }
        }
        ui.small("Файлы safetensors: после загрузки проверяются SHA256, тип и архитектура. Компонент UNet не является полным checkpoint.");
        egui::ScrollArea::vertical()
            .id_salt("hf-files")
            .auto_shrink([false, false])
            .max_height(220.0)
            .show(ui, |ui| {
                let width = (ui.available_width()
                    - egui::Frame::group(ui.style()).total_margin().sum().x)
                    .max(0.0);
                for file in repo
                    .siblings
                    .iter()
                    .filter(|f| f.rfilename.ends_with(".safetensors"))
                {
                    ui.group(|ui| {
                        ui.set_width(width);
                        ui.add(
                            egui::Label::new(egui::RichText::new(&file.rfilename).strong())
                                .truncate(),
                        );
                        ui.horizontal(|ui| {
                            let size = file.lfs.as_ref().map(|l| l.size).or(file.size).unwrap_or(0);
                            ui.label(format!("{:.1} MiB", size as f64 / 1_048_576.0));
                            let component = kind == Kind::Model
                                && (file.rfilename.starts_with("unet/")
                                    || file.rfilename.starts_with("text_encoder")
                                    || file.rfilename.starts_with("transformer/")
                                    || file.rfilename.starts_with("vae/"));
                            let compatible = kind != Kind::Model
                                || self.generation_ui.hf.family != Family::Unknown;
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if primary(
                                        ui,
                                        !busy && !component && compatible && file.lfs.is_some(),
                                        "Загрузить",
                                    )
                                    .clicked()
                                    {
                                        download = Some(file.clone());
                                    }
                                    if component {
                                        ui.weak("Компонент");
                                    }
                                },
                            );
                        });
                    });
                }
            });
        (download, bundle)
    }
    fn hf_results_ui(&self, ui: &mut egui::Ui, busy: bool) -> (Option<String>, bool) {
        let mut chosen = None;
        ui.small("Поиск или owner/repo · SD 1.5 / SDXL · checkpoint, LoRA, VAE и стандартные Diffusers-репозитории");
        egui::ScrollArea::vertical()
            .id_salt("hf-search")
            .auto_shrink([false, false])
            .max_height(290.0)
            .show(ui, |ui| {
                let width = (ui.available_width()
                    - egui::Frame::group(ui.style()).total_margin().sum().x)
                    .max(0.0);
                for model in &self.generation_ui.hf.models {
                    ui.group(|ui| {
                        ui.set_width(width);
                        ui.add(
                            egui::Label::new(egui::RichText::new(&model.id).strong()).truncate(),
                        );
                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "{} загрузок · {:?}",
                                model.downloads,
                                huggingface::family(&model.tags, &serde_json::Value::Null)
                            ));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if primary(ui, !busy, "Файлы / выбор").clicked() {
                                        chosen = Some(model.id.clone());
                                    }
                                },
                            );
                        });
                    });
                }
            });
        let next = ui
            .add_enabled(
                !busy && self.generation_ui.hf.next.is_some(),
                egui::Button::new("Следующая страница"),
            )
            .clicked();
        (chosen, next)
    }
    fn hf_progress(&mut self, ui: &mut egui::Ui) {
        let hf = &mut self.generation_ui.hf;
        if let Some(worker) = &hf.worker {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(&worker.label);
                if ui.button("Отмена").clicked() {
                    worker.cancelled.store(true, Ordering::Release);
                }
            });
            if let Some((n, total)) = hf.progress {
                ui.label(format!(
                    "{:.1} MiB{}",
                    n as f64 / 1_048_576.0,
                    total.map_or_else(String::new, |t| format!(
                        " / {:.1} MiB",
                        t as f64 / 1_048_576.0
                    ))
                ));
                if let Some(total) = total.filter(|t| *t > 0) {
                    ui.add(egui::ProgressBar::new(n as f32 / total as f32));
                }
            }
        }
    }
}
fn primary(ui: &mut egui::Ui, enabled: bool, label: &str) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(egui::RichText::new(label).color(Color32::WHITE))
            .fill(ACCENT)
            .min_size(egui::vec2(160.0, 32.0)),
    )
}
