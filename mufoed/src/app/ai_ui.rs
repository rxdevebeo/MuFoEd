//! The AI tab's side of the window: what the user asked for, the job that
//! runs it, the pane its results show in, and the setup of Ollama and a
//! model when either is missing.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;

use eframe::egui::{self, Color32, FontFamily, RichText};

use super::{App, Dialog};
use crate::ai;
use crate::icons::{self, Icon};
use crate::theme::{self, Palette};
use crate::worker::{AiSource, Request};

/// What the AI tab was asked to do.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum AiAction {
    /// A summary of the selection (`false`) or the whole document.
    Summary {
        whole: bool,
    },
    /// The selection translated, shown to be put in.
    TranslateSelection,
    /// Every paragraph translated in place.
    TranslateDocument,
    /// Each paragraph beside its translation, in the pane.
    Parallel,
    /// The selection rewritten as asked.
    Rewrite(String),
    /// Paragraph styles chosen by kind.
    Styles,
    /// A judgement of the writing.
    Assess,
    /// Every paragraph brought to the prevailing style, offered to apply.
    Unify,
    Cleanup(crate::cleanup::Options),
}

impl AiAction {
    /// Whether it works on the whole document regardless of the selection.
    fn whole(&self) -> bool {
        matches!(
            self,
            Self::Summary { whole: true }
                | Self::TranslateDocument
                | Self::Parallel
                | Self::Styles
                | Self::Assess
                | Self::Unify
                | Self::Cleanup(_)
        )
    }

    fn title_key(&self) -> &'static str {
        match self {
            Self::Summary { .. } => "ai.summary",
            Self::TranslateSelection | Self::TranslateDocument => "ai.translate",
            Self::Parallel => "ai.parallel",
            Self::Rewrite(_) => "ai.rewrite",
            Self::Styles => "ai.styles",
            Self::Assess => "ai.assess",
            Self::Unify => "ai.unify",
            Self::Cleanup(_) => "cleanup.title",
        }
    }
}

/// Whether Ollama answers and with which models.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum Probe {
    #[default]
    Unknown,
    Checking,
    /// It answers; the models it has.
    Ready(Vec<String>),
    /// It does not answer: why.
    Absent(String),
}

/// A job running or finished.
pub(crate) struct Job {
    cleanup: Option<crate::cleanup::Plan>,
    cleanup_pending: bool,
    action: AiAction,
    tab: u64,
    source: AiSource,
    /// The source paragraphs sent, by index: where each one is.
    items: Vec<usize>,
    events: Receiver<ai::Event>,
    cancel: Arc<AtomicBool>,
    progress: (usize, usize),
    running: bool,
    /// The answer, for jobs with one.
    text: Option<String>,
    /// Each sent paragraph's new text.
    paragraphs: Vec<Option<String>>,
    error: Option<String>,
    /// The results were put into the document.
    applied: bool,
}

/// A download Ollama or its installer is doing.
pub(crate) struct Download {
    what: String,
    rx: Receiver<DownloadEvent>,
    cancel: Arc<AtomicBool>,
    status: String,
    done: u64,
    total: u64,
}

pub(crate) enum DownloadEvent {
    Progress(String, u64, u64),
    Done(Result<Option<std::path::PathBuf>, String>),
}

/// A translated picture as PNG, or why there is none.
type PictureResult = Result<Vec<u8>, String>;

/// Everything the AI side keeps.
#[derive(Default)]
pub(crate) struct AiState {
    probe: Probe,
    probe_rx: Option<Receiver<Result<Vec<String>, String>>>,
    /// What waits for the document's text: request id, its tab, the action.
    pending: Option<(u64, u64, AiAction)>,
    /// The tab whose AI pane is open. Other documents do not show it.
    pub(crate) shown: Option<u64>,
    next_id: u64,
    job: Option<Job>,
    download: Option<Download>,
    /// A fetched Windows installer, to run.
    installer: Option<std::path::PathBuf>,
    /// The model to fetch, chosen in the setup dialog.
    pull_choice: usize,
    /// An action to run once Ollama and a model are there.
    waiting: Option<AiAction>,
    /// The vision model the picture translation needs is missing.
    need_vision: bool,
    /// A picture being translated: its tab and drawing, and the result.
    picture: Option<(u64, u32, Receiver<PictureResult>)>,
}

impl App {
    pub(crate) fn ai_cleanup_result(&mut self, tab: u64, applied: bool) {
        if let Some(job) = self
            .ai
            .job
            .as_mut()
            .filter(|job| job.tab == tab && job.cleanup_pending)
        {
            job.cleanup_pending = false;
            job.applied = applied;
        }
    }
    fn ai_config(&self) -> Option<ai::Config> {
        let AiState { probe, .. } = &self.ai;
        let Probe::Ready(models) = probe else {
            return None;
        };
        let model = ai::choose(&self.settings.ai_model, models)?;
        Some(ai::Config {
            endpoint: self.settings.ocr_endpoint.clone(),
            model,
        })
    }

    /// Asks Ollama what it has, off the interface thread.
    pub(crate) fn ai_probe(&mut self, ctx: &egui::Context) {
        if self.ai.probe == Probe::Checking {
            return;
        }
        self.ai.probe = Probe::Checking;
        let (tx, rx) = std::sync::mpsc::channel();
        let endpoint = self.settings.ocr_endpoint.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(ai::models(&endpoint));
            ctx.request_repaint();
        });
        self.ai.probe_rx = Some(rx);
    }

    /// Starts an action: the document's text is asked for, or Ollama set
    /// up first.
    pub(crate) fn ai_run(&mut self, ctx: &egui::Context, action: AiAction) {
        if self.ai_config().is_none()
            && !matches!(&action, AiAction::Cleanup(options) if !options.joins)
        {
            self.ai.waiting = Some(action);
            if !matches!(self.ai.probe, Probe::Ready(_)) {
                self.ai_probe(ctx);
            }
            self.dialog = Some(Dialog::AiSetup);
            return;
        }
        if self
            .ai
            .job
            .as_ref()
            .is_some_and(|job| job.running || job.cleanup_pending)
        {
            let text = self.t("ai.busy").to_owned();
            self.say(text, true, ctx.input(|input| input.time));
            return;
        }
        self.ai.next_id += 1;
        let id = self.ai.next_id;
        let Some(tab) = self.active_tab() else {
            return;
        };
        let tab_id = tab.id;
        tab.send(if matches!(action, AiAction::Cleanup(_)) {
            Request::CleanupSource { job: id }
        } else {
            Request::AiSource {
                whole: action.whole(),
                job: id,
            }
        });
        self.ai.pending = Some((id, tab_id, action));
        self.ai.shown = Some(tab_id);
    }

    /// The document's text arrived: the job starts.
    pub(crate) fn ai_source(&mut self, ctx: &egui::Context, tab: u64, id: u64, source: AiSource) {
        let Some((pending, pending_tab, action)) = self.ai.pending.take() else {
            return;
        };
        if pending != id {
            self.ai.pending = Some((pending, pending_tab, action));
            return;
        }
        let now = ctx.input(|input| input.time);
        let config = match self.ai_config() {
            Some(config) => config,
            None if matches!(&action, AiAction::Cleanup(options) if !options.joins) => ai::Config {
                endpoint: String::new(),
                model: String::new(),
            },
            None => return,
        };
        let selected = source.selection.as_ref().map(|(_, _, text)| text.clone());
        let needs_selection = matches!(action, AiAction::TranslateSelection | AiAction::Rewrite(_));
        if needs_selection && selected.is_none() {
            let text = self.t("ai.select_first").to_owned();
            self.say(text, true, now);
            return;
        }
        // The texts the model gets, and which paragraphs they are.
        let (texts, items): (Vec<String>, Vec<usize>) = match (&action, &selected) {
            (AiAction::Cleanup(_), _) => source
                .paragraphs
                .iter()
                .enumerate()
                .map(|(i, (_, text))| (text.clone(), i))
                .unzip(),
            (
                AiAction::TranslateSelection
                | AiAction::Rewrite(_)
                | AiAction::Summary { whole: false },
                Some(text),
            ) => (vec![text.clone()], Vec::new()),
            _ => source
                .paragraphs
                .iter()
                .enumerate()
                .filter(|(_, (_, text))| {
                    !text.trim().is_empty() && !text.contains(crate::nav::OBJECT)
                })
                .map(|(index, (_, text))| (text.clone(), index))
                .unzip(),
        };
        if texts.is_empty() && !matches!(action, AiAction::Cleanup(_)) {
            let text = self.t("ai.nothing").to_owned();
            self.say(text, true, now);
            return;
        }
        let task = match &action {
            AiAction::Summary { .. } => ai::Task::Summary,
            AiAction::TranslateSelection | AiAction::TranslateDocument | AiAction::Parallel => {
                ai::Task::Translate {
                    language: self.settings.ai_language.clone(),
                }
            }
            AiAction::Rewrite(instruction) => ai::Task::Rewrite {
                instruction: instruction.clone(),
            },
            AiAction::Styles => ai::Task::Styles,
            AiAction::Assess => ai::Task::Assess,
            AiAction::Unify => ai::Task::Unify,
            AiAction::Cleanup(options) => ai::Task::Cleanup {
                joins: options.joins,
            },
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let count = texts.len();
        ai::start(
            config,
            task,
            texts,
            self.i18n.language(),
            tx,
            Arc::clone(&cancel),
            ctx.clone(),
        );
        self.ai.job = Some(Job {
            cleanup: None,
            cleanup_pending: false,
            action,
            tab,
            source,
            items,
            events: rx,
            cancel,
            progress: (0, count),
            running: true,
            text: None,
            paragraphs: vec![None; count],
            error: None,
            applied: false,
        });
    }

    /// Whether Ollama has a text model and the vision model; if not, the
    /// setup dialog says what is missing.
    pub(crate) fn ai_ready_for_pictures(&mut self, ctx: &egui::Context) -> bool {
        let vision = self.settings.ocr_model.clone();
        let has_vision = matches!(&self.ai.probe, Probe::Ready(models) if models.contains(&vision));
        if self.ai_config().is_some() && has_vision {
            self.ai.need_vision = false;
            return true;
        }
        self.ai.need_vision = !has_vision;
        if !matches!(self.ai.probe, Probe::Ready(_)) {
            self.ai_probe(ctx);
        }
        self.dialog = Some(Dialog::AiSetup);
        false
    }

    /// Reads the picture's lines, translates them and draws them back in,
    /// on a thread; the picture is replaced when it is done.
    pub(crate) fn translate_picture(
        &mut self,
        ctx: &egui::Context,
        tab: u64,
        id: u32,
        bytes: Vec<u8>,
    ) {
        let Some(text) = self.ai_config() else {
            return;
        };
        let vision = ai::Config {
            endpoint: text.endpoint.clone(),
            model: self.settings.ocr_model.clone(),
        };
        let language = self.settings.ai_language.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let pixmap = crate::raster::picture_pixmap(&bytes).ok_or("not a picture")?;
                let png = pixmap.encode_png().map_err(|error| error.to_string())?;
                let boxes = ai::find_text(&vision, &png, pixmap.width(), pixmap.height())?;
                if boxes.is_empty() {
                    return Err("no text".to_owned());
                }
                let lines: Vec<String> = boxes.iter().map(|b| b.text.clone()).collect();
                let translated = ai::translate_lines(&text, &lines, &language)?;
                let labels: Vec<crate::raster::Relabel> = boxes
                    .iter()
                    .zip(translated)
                    .map(|(found, text)| crate::raster::Relabel {
                        rect: found.rect,
                        text,
                    })
                    .collect();
                crate::raster::picture_relabelled(&bytes, &labels)
                    .ok_or_else(|| "render".to_owned())
            })();
            let _ = tx.send(result);
            ctx2.request_repaint();
        });
        self.ai.picture = Some((tab, id, rx));
        let now = ctx.input(|input| input.time);
        let note = self.t("ai.picture_working").to_owned();
        self.say(note, false, now);
    }

    /// Takes what the job, the probe and a download report.
    pub(crate) fn ai_poll(&mut self, ctx: &egui::Context) {
        if let Some((tab, id, result)) = self
            .ai
            .picture
            .as_ref()
            .and_then(|(tab, id, rx)| rx.try_recv().ok().map(|result| (*tab, *id, result)))
        {
            self.ai.picture = None;
            let now = ctx.input(|input| input.time);
            match result {
                Ok(bytes) => {
                    if let Some(tab) = self.tabs.iter().find(|t| t.id == tab) {
                        tab.send(Request::Object(crate::worker::ObjectCommand::Replace {
                            id,
                            bytes,
                        }));
                    }
                    let note = self.t("ai.picture_done").to_owned();
                    self.say(note, false, now);
                }
                Err(error) => {
                    let note = self.i18n.tf("ai.picture_failed", &[("error", &error)]);
                    self.say(note, true, now);
                }
            }
        }
        if let Some(result) = self.ai.probe_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.ai.probe_rx = None;
            self.ai.probe = match result {
                Ok(models) => Probe::Ready(models),
                Err(error) => Probe::Absent(error),
            };
            // An action that waited for Ollama runs now that it is there.
            if self.ai_config().is_some() {
                if let Some(action) = self.ai.waiting.take() {
                    if matches!(self.dialog, Some(Dialog::AiSetup)) {
                        self.dialog = None;
                    }
                    self.ai_run(ctx, action);
                }
            }
        }
        self.ai_poll_download(ctx);
        let Some(job) = self.ai.job.as_mut() else {
            return;
        };
        let mut sends = Vec::new();
        while let Ok(event) = job.events.try_recv() {
            match event {
                ai::Event::Progress(done, total) => job.progress = (done, total),
                ai::Event::Text(text) => job.text = Some(text),
                ai::Event::Paragraph(index, text) => {
                    if job.action == AiAction::TranslateDocument {
                        // The document changes as the translation comes.
                        if let Some((at, old)) = job
                            .items
                            .get(index)
                            .and_then(|i| job.source.paragraphs.get(*i))
                        {
                            sends.push(Request::SetParagraphText {
                                at: at.clone(),
                                old: old.clone(),
                                new: text.clone(),
                            });
                            job.applied = true;
                        }
                    }
                    if let Some(slot) = job.paragraphs.get_mut(index) {
                        *slot = Some(text);
                    }
                }
                ai::Event::Kinds(kinds) => {
                    let kinds = kinds
                        .into_iter()
                        .filter_map(|(index, kind)| {
                            let at = job
                                .items
                                .get(index)
                                .and_then(|i| job.source.paragraphs.get(*i))?;
                            Some((at.0.clone(), kind))
                        })
                        .collect();
                    sends.push(Request::ApplyKinds(kinds));
                    job.applied = true;
                }
                ai::Event::CleanupJoins(joins) => {
                    if let (AiAction::Cleanup(options), Some(snapshot)) =
                        (&job.action, &job.source.cleanup)
                    {
                        job.cleanup = Some(crate::cleanup::build(snapshot, options, &joins));
                    }
                }
                ai::Event::Failed(error) => {
                    job.running = false;
                    job.error = Some(error);
                    if let (AiAction::Cleanup(options), Some(snapshot)) =
                        (&job.action, &job.source.cleanup)
                    {
                        job.cleanup = Some(crate::cleanup::build(snapshot, options, &[]));
                    }
                }
                ai::Event::Finished => job.running = false,
            }
        }
        let tab_id = job.tab;
        if let Some(tab) = self.tabs.iter().find(|tab| tab.id == tab_id) {
            for request in sends {
                tab.send(request);
            }
        }
    }

    fn ai_poll_download(&mut self, ctx: &egui::Context) {
        let Some(download) = self.ai.download.as_mut() else {
            return;
        };
        let mut finished = None;
        while let Ok(event) = download.rx.try_recv() {
            match event {
                DownloadEvent::Progress(status, done, total) => {
                    download.status = status;
                    download.done = done;
                    download.total = total;
                }
                DownloadEvent::Done(result) => finished = Some(result),
            }
        }
        if let Some(result) = finished {
            let what = download.what.clone();
            self.ai.download = None;
            let now = ctx.input(|input| input.time);
            match result {
                Ok(Some(installer)) => self.ai.installer = Some(installer),
                Ok(None) => {
                    let text = self.i18n.tf("ai.pulled", &[("model", &what)]);
                    self.say(text, false, now);
                    self.ai_probe(ctx);
                }
                Err(error) => {
                    let text = self.i18n.tf("ai.download_failed", &[("error", &error)]);
                    self.say(text, true, now);
                }
            }
        }
    }

    fn ai_pull(&mut self, ctx: &egui::Context, model: &str) {
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let endpoint = self.settings.ocr_endpoint.clone();
        let name = model.to_owned();
        let stop = Arc::clone(&cancel);
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let progress_tx = tx.clone();
            let mut last = std::time::Instant::now();
            let result = ai::pull(&endpoint, &name, &stop, |status, done, total| {
                if last.elapsed().as_millis() > 200 || status == "success" {
                    last = std::time::Instant::now();
                    let _ =
                        progress_tx.send(DownloadEvent::Progress(status.to_owned(), done, total));
                    ctx2.request_repaint();
                }
            });
            let _ = tx.send(DownloadEvent::Done(result.map(|()| None)));
            ctx2.request_repaint();
        });
        self.ai.download = Some(Download {
            what: model.to_owned(),
            rx,
            cancel,
            status: String::new(),
            done: 0,
            total: 0,
        });
    }

    fn ai_fetch_installer(&mut self, ctx: &egui::Context) {
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancel);
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let progress_tx = tx.clone();
            let mut last = std::time::Instant::now();
            let result = ai::download_installer(&stop, |done, total| {
                if last.elapsed().as_millis() > 200 {
                    last = std::time::Instant::now();
                    let _ = progress_tx.send(DownloadEvent::Progress(String::new(), done, total));
                    ctx2.request_repaint();
                }
            });
            let _ = tx.send(DownloadEvent::Done(result.map(Some)));
            ctx2.request_repaint();
        });
        self.ai.download = Some(Download {
            what: "Ollama".to_owned(),
            rx,
            cancel,
            status: String::new(),
            done: 0,
            total: 0,
        });
    }

    /// The setup dialog: Ollama missing, a model missing, or a model to
    /// choose. Returns whether it stays open.
    pub(crate) fn ai_setup_ui(&mut self, ctx: &egui::Context) -> bool {
        let t = |key: &str| self.i18n.t(key).to_owned();
        let mut close = false;
        let mut probe = false;
        let mut pull: Option<String> = None;
        let mut fetch_installer = false;
        let mut run_installer = None;
        let mut cancel_download = false;
        let mut chosen_model: Option<String> = None;
        let endpoint = self.settings.ocr_endpoint.clone();
        let mut choice = self.ai.pull_choice;
        let state = &self.ai;
        let configured = self.settings.ai_model.clone();
        let response = crate::dialog::modal(ctx, "ai-setup").show(ctx, |ui| {
            crate::dialog::title(ui, &t("ai.setup_title"));
            ui.set_max_width(480.0);
            match &state.probe {
                Probe::Unknown | Probe::Checking => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(t("ai.checking"));
                    });
                }
                Probe::Absent(error) => {
                    ui.label(t("ai.no_ollama"));
                    ui.label(
                        RichText::new(
                            self.i18n
                                .tf("ai.no_answer", &[("endpoint", &endpoint), ("error", error)]),
                        )
                        .weak()
                        .size(12.0),
                    );
                    ui.add_space(4.0);
                    if cfg!(windows) {
                        ui.label(t("ai.install_windows"));
                    } else if cfg!(target_os = "macos") {
                        ui.label(t("ai.install_mac"));
                    } else {
                        ui.label(t("ai.install_linux"));
                        ui.horizontal(|ui| {
                            ui.code(ai::OLLAMA_LINUX);
                            if ui.small_button(t("cmd.copy")).clicked() {
                                ui.ctx().copy_text(ai::OLLAMA_LINUX.to_owned());
                            }
                        });
                    }
                    ui.label(RichText::new(t("ai.start_hint")).weak().size(12.0));
                }
                Probe::Ready(models) => {
                    let text_models: Vec<&String> = models
                        .iter()
                        .filter(|name| ai::choose(name, &[(*name).clone()]).is_some())
                        .collect();
                    if state.need_vision {
                        ui.label(
                            self.i18n
                                .tf("ai.need_vision", &[("model", &self.settings.ocr_model)]),
                        );
                        if ui
                            .add_enabled(state.download.is_none(), egui::Button::new(t("ai.pull")))
                            .clicked()
                        {
                            pull = Some(self.settings.ocr_model.clone());
                        }
                        ui.add_space(4.0);
                    }
                    match ai::choose(&configured, models) {
                        Some(model) => {
                            ui.label(self.i18n.tf("ai.ready", &[("model", &model)]));
                            ui.horizontal(|ui| {
                                ui.label(t("ai.model"));
                                egui::ComboBox::from_id_salt("ai-model")
                                    .selected_text(model.clone())
                                    .width(260.0)
                                    .show_ui(ui, |ui| {
                                        for name in &text_models {
                                            if ui
                                                .selectable_label(**name == model, name.as_str())
                                                .clicked()
                                            {
                                                chosen_model = Some((*name).clone());
                                            }
                                        }
                                    });
                            });
                        }
                        None => {
                            ui.label(t("ai.no_model"));
                        }
                    }
                    ui.add_space(4.0);
                    ui.label(RichText::new(t("ai.more_models")).weak().size(12.0));
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("ai-pull")
                            .selected_text(
                                ai::RECOMMENDED
                                    .get(choice)
                                    .map(|(name, size)| format!("{name} (≈{size:.1} GB)"))
                                    .unwrap_or_default(),
                            )
                            .show_ui(ui, |ui| {
                                for (index, (name, size)) in ai::RECOMMENDED.iter().enumerate() {
                                    ui.selectable_value(
                                        &mut choice,
                                        index,
                                        format!("{name} (≈{size:.1} GB)"),
                                    );
                                }
                            });
                        if ui
                            .add_enabled(state.download.is_none(), egui::Button::new(t("ai.pull")))
                            .clicked()
                        {
                            pull = ai::RECOMMENDED
                                .get(choice)
                                .map(|(name, _)| (*name).to_owned());
                        }
                    });
                }
            }
            if let Some(download) = &state.download {
                ui.add_space(6.0);
                #[allow(clippy::cast_precision_loss)]
                let fraction = if download.total > 0 {
                    download.done as f32 / download.total as f32
                } else {
                    0.0
                };
                #[allow(clippy::cast_precision_loss)]
                let text = format!(
                    "{} {} — {:.0} / {:.0} MB",
                    download.what,
                    download.status,
                    download.done as f64 / 1e6,
                    download.total as f64 / 1e6
                );
                ui.add(
                    egui::ProgressBar::new(fraction)
                        .text(text)
                        .desired_width(440.0),
                );
                if ui.small_button(t("button.cancel")).clicked() {
                    cancel_download = true;
                }
            }
            crate::dialog::buttons(ui, |ui| {
                match &state.probe {
                    Probe::Absent(_) => {
                        if let Some(installer) = &state.installer {
                            if crate::dialog::primary(ui, &t("ai.run_installer")).clicked() {
                                run_installer = Some(installer.clone());
                            }
                        } else if cfg!(windows) {
                            if crate::dialog::primary_enabled(
                                ui,
                                state.download.is_none(),
                                &t("ai.get_ollama"),
                            )
                            .clicked()
                            {
                                fetch_installer = true;
                            }
                        } else if crate::dialog::primary(ui, &t("ai.open_page")).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(ai::OLLAMA_PAGE));
                        }
                        if crate::dialog::button(ui, &t("ai.check_again")).clicked() {
                            probe = true;
                        }
                    }
                    Probe::Ready(_) => {
                        if crate::dialog::primary(ui, &t("ai.check_again")).clicked() {
                            probe = true;
                        }
                    }
                    _ => {}
                }
                let close_button = if matches!(state.probe, Probe::Absent(_) | Probe::Ready(_)) {
                    crate::dialog::button(ui, &t("button.close"))
                } else {
                    crate::dialog::primary(ui, &t("button.close"))
                };
                if close_button.clicked() {
                    close = true;
                }
            });
        });
        self.ai.pull_choice = choice;
        if let Some(model) = chosen_model {
            self.settings.ai_model = model;
            self.settings.save();
        }
        if probe {
            self.ai_probe(ctx);
        }
        if let Some(model) = pull {
            self.ai_pull(ctx, &model);
        }
        if fetch_installer {
            self.ai_fetch_installer(ctx);
        }
        if let Some(installer) = run_installer {
            // The user asked for it: the installer Ollama publishes runs.
            let now = ctx.input(|input| input.time);
            match std::process::Command::new(&installer).spawn() {
                Ok(_) => {
                    let text = self.t("ai.installer_started").to_owned();
                    self.say(text, false, now);
                }
                Err(error) => self.say(error.to_string(), true, now),
            }
            self.ai.installer = None;
        }
        if cancel_download {
            if let Some(download) = &self.ai.download {
                download.cancel.store(true, Ordering::Relaxed);
            }
        }
        if close || response.should_close() {
            self.ai.waiting = None;
            return false;
        }
        true
    }

    /// The pane: the job's progress and its results.
    pub(crate) fn ai_pane(&mut self, ui: &mut egui::Ui, palette: Palette) {
        crate::theme::style_buttons(ui);
        let t = |key: &str| self.i18n.t(key).to_owned();
        let mut closing = false;
        ui.horizontal(|ui| {
            icons::icon_widget(ui, Icon::Sparkle, 16.0, theme::ACCENT);
            ui.label(RichText::new(t("ai.pane")).family(FontFamily::Name(theme::BOLD.into())));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("×")
                    .on_hover_text(t("button.close"))
                    .clicked()
                {
                    closing = true;
                }
                if ui
                    .small_button("⚙")
                    .on_hover_text(t("ai.settings"))
                    .clicked()
                {
                    self.dialog = Some(Dialog::AiSetup);
                }
            });
        });
        if closing {
            self.ai.shown = None;
            return;
        }
        if let Some(config) = self.ai_config() {
            ui.label(
                RichText::new(self.i18n.tf("ai.using", &[("model", &config.model)]))
                    .weak()
                    .size(11.0),
            );
        }
        ui.add_space(6.0);
        let here = self.active_tab().map(|tab| tab.id);
        if self
            .ai
            .pending
            .as_ref()
            .is_some_and(|(_, tab, _)| Some(*tab) == here)
        {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(t("ai.preparing"));
            });
        }
        if self.ai.job.as_ref().is_some_and(|job| {
            Some(job.tab) == here && matches!(job.action, AiAction::Cleanup(_))
        }) {
            let job = self.ai.job.as_mut().unwrap();
            let tab_id = job.tab;
            if let Some(plan) = cleanup_ui(ui, job, &self.i18n) {
                if let Some(tab) = self.tabs.iter().find(|tab| tab.id == tab_id) {
                    tab.send(Request::ApplyCleanup(Box::new(plan)));
                }
            }
            return;
        }
        let Some(job) = self.ai.job.as_ref().filter(|job| Some(job.tab) == here) else {
            if self
                .ai
                .pending
                .as_ref()
                .is_none_or(|(_, tab, _)| Some(*tab) != here)
            {
                ui.label(RichText::new(t("ai.empty")).weak());
            }
            return;
        };
        ui.label(RichText::new(t(job.action.title_key())).strong());
        let mut cancel = false;
        if job.running {
            ui.horizontal(|ui| {
                let (done, total) = job.progress;
                #[allow(clippy::cast_precision_loss)]
                let fraction = if total > 0 {
                    done as f32 / total as f32
                } else {
                    0.0
                };
                if total > 1 {
                    ui.add(
                        egui::ProgressBar::new(fraction)
                            .text(format!("{done} / {total}"))
                            .desired_width(ui.available_width() - 80.0),
                    );
                } else {
                    ui.spinner();
                    ui.label(t("ai.thinking"));
                }
                if ui.small_button(t("button.cancel")).clicked() {
                    cancel = true;
                }
            });
        }
        if let Some(error) = &job.error {
            ui.colored_label(Color32::from_rgb(200, 50, 50), error);
        }
        let mut requests = Vec::new();
        let mut copy = None;
        let mut applied = false;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // One answer: the text, and what can be done with it.
                let answer = job.text.clone().or_else(|| match job.action {
                    AiAction::TranslateSelection => job.paragraphs.first().cloned().flatten(),
                    _ => None,
                });
                if let Some(text) = answer {
                    let frame = egui::Frame::new()
                        .fill(palette.button)
                        .corner_radius(6)
                        .inner_margin(10)
                        .stroke(egui::Stroke::new(1.0, palette.ribbon_line));
                    frame.show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(egui::Label::new(RichText::new(&text).size(13.0)).selectable(true));
                    });
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        let selection = job.source.selection.clone();
                        let replacing = matches!(
                            job.action,
                            AiAction::TranslateSelection | AiAction::Rewrite(_)
                        );
                        if let (Some((start, end, old)), true) = (selection, replacing) {
                            if crate::dialog::primary(ui, &t("ai.replace_selection")).clicked() {
                                requests.push(Request::ReplaceRange {
                                    start,
                                    end,
                                    old,
                                    new: text.clone(),
                                });
                                applied = true;
                            }
                        }
                        let insert = if replacing {
                            crate::dialog::button(ui, &t("ai.insert_at_caret"))
                        } else {
                            crate::dialog::primary(ui, &t("ai.insert_at_caret"))
                        };
                        if insert.clicked() {
                            requests.push(Request::Paste(format!("{text}\n")));
                            applied = true;
                        }
                        if crate::dialog::button(ui, &t("cmd.copy")).clicked() {
                            copy = Some(text.clone());
                        }
                    });
                }
                // Paragraph by paragraph: beside each other, or to apply.
                if matches!(
                    job.action,
                    AiAction::Parallel | AiAction::Unify | AiAction::TranslateDocument
                ) {
                    if job.action == AiAction::Unify && !job.running && !job.applied {
                        let changed = job
                            .paragraphs
                            .iter()
                            .zip(&job.items)
                            .filter(|(new, index)| {
                                new.as_ref().is_some_and(|new| {
                                    job.source
                                        .paragraphs
                                        .get(**index)
                                        .is_some_and(|(_, old)| old.trim() != new.trim())
                                })
                            })
                            .count();
                        ui.label(self.i18n.tf("ai.changed", &[("count", &changed)]));
                        if crate::dialog::primary(ui, &t("ai.apply_all")).clicked() {
                            for (new, index) in job.paragraphs.iter().zip(&job.items) {
                                let (Some(new), Some((at, old))) =
                                    (new, job.source.paragraphs.get(*index))
                                else {
                                    continue;
                                };
                                if old.trim() != new.trim() {
                                    requests.push(Request::SetParagraphText {
                                        at: at.clone(),
                                        old: old.clone(),
                                        new: new.clone(),
                                    });
                                }
                            }
                            applied = true;
                        }
                        ui.add_space(6.0);
                    }
                    for (new, index) in job.paragraphs.iter().zip(&job.items) {
                        let Some((at, old)) = job.source.paragraphs.get(*index) else {
                            continue;
                        };
                        if job.action == AiAction::Unify
                            && new.as_ref().is_some_and(|new| new.trim() == old.trim())
                        {
                            continue;
                        }
                        let frame = egui::Frame::new()
                            .fill(palette.button)
                            .corner_radius(4)
                            .inner_margin(8)
                            .stroke(egui::Stroke::new(1.0, palette.ribbon_line));
                        let response = frame
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(RichText::new(old).size(12.0).weak());
                                ui.add_space(2.0);
                                match new {
                                    Some(new) => ui.label(RichText::new(new).size(13.0)),
                                    None => ui.label(RichText::new("…").weak()),
                                };
                            })
                            .response
                            .interact(egui::Sense::click());
                        if response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(t("ai.go_to")).clicked() {
                            requests.push(Request::GoTo(at.clone()));
                        }
                        ui.add_space(4.0);
                    }
                }
                if job.action == AiAction::Styles && !job.running && job.error.is_none() {
                    ui.label(t("ai.styles_done"));
                }
            });
        let tab_id = job.tab;
        if cancel {
            job.cancel.store(true, Ordering::Relaxed);
        }
        if let Some(text) = copy {
            ui.ctx().copy_text(text);
        }
        if let Some(tab) = self.tabs.iter().find(|tab| tab.id == tab_id) {
            for request in requests {
                tab.send(request);
            }
        }
        if applied {
            if let Some(job) = self.ai.job.as_mut() {
                job.applied = true;
            }
        }
    }
}

fn cleanup_ui(
    ui: &mut egui::Ui,
    job: &mut Job,
    i18n: &crate::i18n::I18n,
) -> Option<crate::cleanup::Plan> {
    ui.strong(i18n.t("cleanup.title"));
    if job.running {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(format!("{} / {}", job.progress.0, job.progress.1));
            if ui.button(i18n.t("button.cancel")).clicked() {
                job.cancel.store(true, Ordering::Relaxed);
            }
        });
    }
    if let Some(error) = &job.error {
        ui.colored_label(Color32::from_rgb(180, 70, 30), i18n.t("cleanup.ai_failed"));
        ui.collapsing(i18n.t("cleanup.details"), |ui| {
            ui.label(error);
        });
    }
    let plan = job.cleanup.as_mut()?;
    ui.label(i18n.tf(
        "cleanup.found",
        &[("count", &plan.changes.len()), ("skipped", &plan.skipped)],
    ));
    ui.label(egui::RichText::new(i18n.t("cleanup.review_hint")).weak());
    let mut apply = false;
    if job.applied {
        ui.label(i18n.t("cleanup.applied"));
    }
    if !job.applied {
        let selected = plan.changes.iter().any(|c| c.selected);
        apply = crate::dialog::primary_enabled(
            ui,
            selected && !job.running && !job.cleanup_pending,
            i18n.t("cleanup.apply"),
        )
        .clicked();
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        for change in &mut plan.changes {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.add_enabled_ui(!job.applied && !job.cleanup_pending, |ui| {
                    ui.checkbox(&mut change.selected, i18n.t(change.key));
                });
                let before = change
                    .before
                    .iter()
                    .map(crate::cleanup::text)
                    .collect::<Vec<_>>()
                    .join("\n¶\n");
                let after = change
                    .after
                    .as_ref()
                    .map_or_else(|| i18n.t("cleanup.deleted").into(), crate::cleanup::text);
                ui.label(i18n.t("cleanup.before"));
                ui.label(
                    RichText::new(visible_spaces(&before, i18n.t("cleanup.tab_marker"))).weak(),
                );
                ui.label(i18n.t("cleanup.after"));
                ui.label(visible_spaces(&after, i18n.t("cleanup.tab_marker")));
                if change
                    .after
                    .as_ref()
                    .is_some_and(|p| p.props.indentation != change.before[0].props.indentation)
                {
                    ui.label(i18n.t("cleanup.indent_changed"));
                }
            });
        }
    });
    if apply {
        job.cleanup_pending = true;
        Some(plan.clone())
    } else {
        None
    }
}

fn visible_spaces(text: &str, tab: &str) -> String {
    let mut result = String::new();
    for c in text.chars().take(600) {
        match c {
            ' ' => result.push('·'),
            '\t' => result.push_str(tab),
            other => result.push(other),
        }
    }
    if text.chars().count() > 600 {
        result.push('…');
    }
    result
}
