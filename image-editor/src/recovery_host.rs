//! Demo host: bounded background checkpointing and a blocking startup choice.
//! The widget has no implicit filesystem writes; `MuFoEd` can use `recovery::Session` directly.
use image_editor::{
    recovery::{self, Candidate, Draft, Session},
    ui::ImageEditor,
    Document,
};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const INTERVAL: Duration = Duration::from_secs(10);
enum Command {
    Save {
        document: Document,
        source: Option<PathBuf>,
        epoch: u64,
    },
    Clear,
}
enum Outcome {
    Saved(u64),
    Cleared,
}
struct Writer {
    send: SyncSender<Command>,
    receive: Receiver<Result<Outcome, String>>,
    busy: bool,
    has_file: bool,
}
impl Writer {
    fn new(path: &Path) -> Result<Self, String> {
        let session = Session::new(path).map_err(|e| e.to_string())?;
        let (send, commands) = mpsc::sync_channel(1);
        let (results, receive) = mpsc::channel();
        std::thread::Builder::new()
            .name("image-recovery-writer".into())
            .spawn(move || {
                let mut saved = None;
                while let Ok(command) = commands.recv() {
                    let result = match command {
                        Command::Save {
                            document,
                            source,
                            epoch,
                        } => (|| {
                            let signature = recovery::fingerprint(&document, source.as_deref())?;
                            if saved != Some(signature) {
                                session.checkpoint(&document, source.as_deref())?;
                                saved = Some(signature);
                            }
                            Ok::<_, image_editor::EditError>(Outcome::Saved(epoch))
                        })(),
                        Command::Clear => session.clear().map(|()| {
                            saved = None;
                            Outcome::Cleared
                        }),
                    }
                    .map_err(|e| e.to_string());
                    if results.send(result).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            send,
            receive,
            busy: false,
            has_file: false,
        })
    }
    fn submit(&mut self, command: Command) -> Result<(), String> {
        self.send.try_send(command).map_err(|e| e.to_string())?;
        self.busy = true;
        Ok(())
    }
}
struct Loaded {
    candidate: Candidate,
    draft: Result<Draft, String>,
    preview: Option<image_editor::Raster>,
    texture: Option<egui::TextureHandle>,
}
pub(super) struct RecoveryHost {
    writer: Option<Writer>,
    epoch: u64,
    source: Option<PathBuf>,
    next: Instant,
    interval: Duration,
    clear_pending: bool,
    observed_dirty: bool,
    closing: bool,
    notice: Option<String>,
    pending: VecDeque<Candidate>,
    loading: Option<Receiver<Loaded>>,
    loaded: Option<Loaded>,
    backup: Option<(u64, Candidate)>,
}
pub(super) fn path(preferences: &Path) -> PathBuf {
    std::env::var_os("IMAGE_STUDIO_RECOVERY_DIR")
        .map_or_else(|| preferences.with_extension("recovery"), PathBuf::from)
}
impl RecoveryHost {
    pub(super) fn new(directory: &Path, epoch: u64, source: Option<PathBuf>) -> Self {
        Self::create(directory, epoch, source, true)
    }
    /// A document tab gets its own writer but must not rediscover startup copies.
    pub(super) fn new_tab(directory: &Path, epoch: u64, source: Option<PathBuf>) -> Self {
        Self::create(directory, epoch, source, false)
    }
    fn create(directory: &Path, epoch: u64, source: Option<PathBuf>, discover: bool) -> Self {
        let mut notice = None;
        let pending = match if discover {
            recovery::discover(directory)
        } else {
            Ok(Vec::new())
        } {
            Ok(c) => c.into(),
            Err(e) => {
                notice = Some(format!("Поиск копий восстановления: {e}"));
                VecDeque::new()
            }
        };
        let writer = match Writer::new(directory) {
            Ok(w) => Some(w),
            Err(e) => {
                notice = Some(format!("Автовосстановление недоступно: {e}"));
                None
            }
        };
        Self {
            writer,
            epoch,
            source,
            next: Instant::now() + INTERVAL,
            interval: INTERVAL,
            clear_pending: false,
            observed_dirty: false,
            closing: false,
            notice,
            pending,
            loading: None,
            loaded: None,
            backup: None,
        }
    }
    /// After restoring one copy, let a fresh prompt own the remaining copies.
    pub(super) fn transfer_pending_to(&mut self, next: &mut Self) {
        next.pending = std::mem::take(&mut self.pending);
    }
    pub(super) fn bind_source(&mut self, epoch: u64, source: Option<PathBuf>) {
        if self.epoch != epoch {
            self.clear_pending = true;
            self.discard_backup();
            self.next = Instant::now();
        }
        self.epoch = epoch;
        self.source = source;
    }
    fn discard_backup(&mut self) {
        if let Some((_, candidate)) = self.backup.take() {
            if let Err(e) = candidate.discard() {
                self.notice = Some(format!(
                    "Не удалось удалить прежнюю копию восстановления: {e}"
                ));
            }
        }
    }
    pub(super) fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }
    pub(super) fn screenshot_ready(&self) -> bool {
        !self.prompt_pending() || self.loaded.is_some()
    }
    pub(super) fn prompt_pending(&self) -> bool {
        self.loading.is_some() || self.loaded.is_some() || !self.pending.is_empty()
    }
    fn poll_load(&mut self) {
        if let Some(receiver) = &self.loading {
            match receiver.try_recv() {
                Ok(loaded) => {
                    self.loaded = Some(loaded);
                    self.loading = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.loading = None;
                    self.notice = Some(
                        "Не удалось прочитать копию восстановления. Она сохранена на диске".into(),
                    );
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if self.loaded.is_none() && self.loading.is_none() && !self.closing {
            if let Some(candidate) = self.pending.pop_front() {
                let (send, receive) = mpsc::channel();
                match std::thread::Builder::new()
                    .name("image-recovery-reader".into())
                    .spawn(move || {
                        let draft = candidate.load().map_err(|e| e.to_string());
                        let preview = draft
                            .as_ref()
                            .ok()
                            .and_then(|d| d.document.flatten().thumbnail(256).ok());
                        let _ = send.send(Loaded {
                            candidate,
                            draft,
                            preview,
                            texture: None,
                        });
                    }) {
                    Ok(_) => self.loading = Some(receive),
                    Err(e) => self.notice = Some(format!("Копия оставлена на диске: {e}")),
                }
            }
        }
    }
    pub(super) fn poll(&mut self, editor: &mut ImageEditor) {
        if self.epoch != editor.project_epoch() {
            self.bind_source(editor.project_epoch(), None);
        }
        self.poll_load();
        if self.observed_dirty && !editor.has_unsaved_changes() {
            self.clear_pending = true;
        }
        self.observed_dirty = editor.has_unsaved_changes();
        let mut outcome = None;
        let mut disconnected = false;
        if let Some(writer) = &mut self.writer {
            if writer.busy {
                match writer.receive.try_recv() {
                    Ok(result) => {
                        writer.busy = false;
                        outcome = Some(result);
                    }
                    Err(TryRecvError::Disconnected) => disconnected = true,
                    Err(TryRecvError::Empty) => {}
                }
            }
        }
        if disconnected {
            self.writer = None;
            self.notice = Some(
                "Поток автовосстановления остановлен. Последняя полная копия сохранена".into(),
            );
            self.clear_pending = false;
        }
        if let Some(outcome) = outcome {
            self.completed(outcome, editor);
        }
        let clean = !editor.has_unsaved_changes() || self.closing;
        if clean {
            self.discard_backup();
        }
        let mut submit_failed = false;
        if !self.prompt_pending() {
            if let Some(writer) = &mut self.writer {
                if !writer.busy {
                    let command = if self.clear_pending
                        || (clean && writer.has_file && Instant::now() >= self.next)
                    {
                        self.clear_pending = false;
                        Some(Command::Clear)
                    } else if !clean && Instant::now() >= self.next {
                        self.next = Instant::now() + self.interval;
                        Some(Command::Save {
                            document: editor.document().clone(),
                            source: self.source.clone(),
                            epoch: self.epoch,
                        })
                    } else {
                        None
                    };
                    if let Some(command) = command {
                        if let Err(e) = writer.submit(command) {
                            self.notice = Some(format!("Автовосстановление: {e}"));
                            submit_failed = true;
                        }
                    }
                }
            }
        }
        if submit_failed {
            self.writer = None;
            self.clear_pending = false;
        }
        if let Some(notice) = self.notice.take() {
            editor.set_status(notice);
        }
    }
    fn completed(&mut self, outcome: Result<Outcome, String>, editor: &ImageEditor) {
        self.next = Instant::now() + self.interval;
        match outcome {
            Ok(Outcome::Saved(epoch)) => {
                if !editor.has_unsaved_changes() || self.closing {
                    self.clear_pending = true;
                }
                if let Some(writer) = &mut self.writer {
                    writer.has_file = true;
                }
                if self.backup.as_ref().is_some_and(|(e, _)| *e == epoch) {
                    self.discard_backup();
                }
            }
            Ok(Outcome::Cleared) => {
                if let Some(writer) = &mut self.writer {
                    writer.has_file = false;
                }
                self.next = Instant::now();
            }
            Err(e) => {
                // Do not block clean shutdown forever on an unwritable cache.
                if self.closing {
                    if let Some(writer) = &mut self.writer {
                        writer.has_file = false;
                    }
                    self.clear_pending = false;
                }
                self.notice = Some(format!(
                    "Автовосстановление: {e}. Исходный проект не изменён"
                ));
            }
        }
    }
    pub(super) fn cancel_close(&mut self) {
        self.closing = false;
        self.next = Instant::now();
    }
    pub(super) fn close(&mut self) {
        self.closing = true;
        self.clear_pending = true;
        self.pending.clear();
        self.loaded = None;
        self.loading = None;
        self.discard_backup();
    }
    pub(super) fn ready_to_close(&self) -> bool {
        self.closing
            && self
                .writer
                .as_ref()
                .is_none_or(|w| !w.busy && !w.has_file && !self.clear_pending)
    }
    fn defer(&mut self) {
        self.loading = None;
        self.loaded = None;
        self.pending.clear();
    }
    fn restore(&mut self, editor: &mut ImageEditor) {
        let Some(loaded) = self.loaded.take() else {
            return;
        };
        match loaded.draft {
            Ok(draft) => {
                editor.restore_recovered(draft.document);
                self.bind_source(editor.project_epoch(), draft.source);
                self.backup = Some((editor.project_epoch(), loaded.candidate));
                self.next = Instant::now();
                editor.set_status(
                    "Проект восстановлен. Сохраните его как PSD; исходный файл не изменён",
                );
            }
            Err(e) => editor.set_status(format!("Восстановление: {e}")),
        }
    }
    fn discard_current(&mut self) {
        if let Some(loaded) = &self.loaded {
            match loaded.candidate.discard() {
                Ok(()) => self.loaded = None,
                Err(e) => self.notice = Some(format!("Не удалось удалить копию: {e}")),
            }
        }
    }
    pub(super) fn show_prompt(&mut self, ctx: &egui::Context, editor: &mut ImageEditor) {
        self.poll_load();
        let mut action = 0;
        let response=egui::Modal::new(egui::Id::new("image-recovery-prompt")).show(ctx,|ui| {
            ui.set_width((ctx.content_rect().width()-60.0).clamp(340.0,560.0));
            ui.heading("Восстановление проекта");
            egui::ScrollArea::vertical().id_salt("recovery-content").min_scrolled_height(if self.loaded.as_ref().is_some_and(|l| l.draft.is_ok()) {360.0} else {130.0}).max_height((ctx.content_rect().height()-200.0).max(200.0)).show(ui,|ui| {
                ui.label("После предыдущего запуска осталась отдельная копия. Исходный PSD не изменён.");
                if let Some(loaded)=&mut self.loaded {
                    match &loaded.draft {
                        Ok(draft)=> {
                            ui.label(draft.source.as_ref().map_or_else(||"Несохранённый проект".into(),|p|p.display().to_string()));
                            ui.small(format!("{} × {} · {} слоёв",draft.document.width(),draft.document.height(),draft.document.layers().len()));
                            let seconds=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs().saturating_sub(draft.saved_at);
                            ui.small(if seconds<60 {"Последняя копия: только что".into()} else {format!("Последняя копия: {} мин назад",seconds/60)});
                            if loaded.texture.is_none() {
                                if let Some(r)=&loaded.preview {loaded.texture=Some(ctx.load_texture("recovery-thumbnail",egui::ColorImage::from_rgba_unmultiplied([r.width() as usize,r.height() as usize],r.pixels()),egui::TextureOptions::LINEAR));}
                            }
                            if let Some(texture)=&loaded.texture {let size=texture.size_vec2();ui.add(egui::Image::new(texture).fit_to_exact_size(size*(190.0/size.x.max(size.y))));}
                        }
                        Err(e)=> {ui.colored_label(egui::Color32::LIGHT_RED,"Копия повреждена или недоступна");ui.label(e);ui.small("Можно продолжить работу с исходным файлом. Копия сохраняется, пока вы явно не удалите её.");}
                    }
                    ui.collapsing("Расположение копии",|ui| {ui.label(loaded.candidate.path().display().to_string());});
                } else {ui.spinner();ui.label("Проверка копии…");}
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(self.loaded.as_ref().is_some_and(|l|l.draft.is_ok()),egui::Button::new("Восстановить").fill(egui::Color32::from_rgb(48, 91, 151))).clicked() {action=1;}
                if ui.add_enabled(self.loaded.is_some(),egui::Button::new("Удалить копию")).clicked() {action=2;}
                if ui.button("Позже").clicked() {action=3;}
            });
        });
        if action == 0 && response.should_close() {
            action = 3;
        }
        match action {
            1 => self.restore(editor),
            2 => self.discard_current(),
            3 => self.defer(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn draft() -> Document {
        let mut d =
            Document::from_raster(image_editor::Raster::solid(8, 6, [80, 100, 120, 128]).unwrap())
                .unwrap();
        d.add_layer_mask(true).unwrap();
        d.set_layer_mask([0, 64, 128, 255].repeat(12)).unwrap();
        d.set_mask(vec![77; 48]).unwrap();
        d.set_selection([0, 128, 255, 0].repeat(12)).unwrap();
        d
    }
    fn abandoned(root: &Path) -> PathBuf {
        let slot = Session::new(root).unwrap();
        slot.checkpoint(&draft(), Some(Path::new("untouched-original.psd")))
            .unwrap();
        let path = slot.path().to_path_buf();
        drop(slot);
        path
    }
    fn pump(
        host: &mut RecoveryHost,
        e: &mut ImageEditor,
        mut done: impl FnMut(&RecoveryHost) -> bool,
    ) {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            host.poll(e);
            if done(host) {
                return;
            }
            assert!(Instant::now() < end, "recovery worker timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn snapshots(root: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(root)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "snapshot"))
            .collect()
    }
    fn loaded(root: &Path) -> (RecoveryHost, ImageEditor) {
        let mut e = ImageEditor::new(Document::new(3, 2).unwrap());
        let mut host = RecoveryHost::new(root, e.project_epoch(), None);
        host.interval = Duration::from_secs(3600);
        pump(&mut host, &mut e, |h| h.loaded.is_some());
        (host, e)
    }
    #[test]
    fn restore_is_unsaved_retains_preferences_and_keeps_old_copy_until_durable_replacement() {
        let root = tempfile::tempdir().unwrap();
        let prior = abandoned(root.path());
        let (mut host, mut e) = loaded(root.path());
        let mut value = serde_json::to_value(e.settings()).unwrap();
        value["request"]["steps"] = 23.into();
        value["request"]["guidance"] = 7.25.into();
        value["request"]["model"] = "D:/Models/retained.safetensors".into();
        e.apply_settings(serde_json::from_value(value).unwrap())
            .unwrap();
        let settings = e.settings();
        assert_eq!(
            serde_json::to_value(&settings).unwrap()["request"]["steps"],
            23
        );
        let epoch = e.project_epoch();
        host.restore(&mut e);
        assert_eq!(e.document(), &draft());
        assert!(e.has_unsaved_changes());
        assert_ne!(e.project_epoch(), epoch);
        assert_eq!(e.settings(), settings);
        assert!(prior.exists());
        assert!(!host.prompt_pending());
        pump(&mut host, &mut e, |h| {
            h.writer.as_ref().is_some_and(|w| !w.busy && w.has_file)
        });
        assert!(!prior.exists());
        assert_eq!(snapshots(root.path()).len(), 1);
        e.mark_saved();
        pump(&mut host, &mut e, |h| {
            h.writer.as_ref().is_some_and(|w| !w.busy && !w.has_file)
        });
        assert!(snapshots(root.path()).is_empty());
        host.close();
        pump(&mut host, &mut e, RecoveryHost::ready_to_close);
    }
    #[test]
    fn failed_new_checkpoint_retains_recovered_backup_and_other_live_window() {
        let root = tempfile::tempdir().unwrap();
        let prior = abandoned(root.path());
        let other = Session::new(root.path()).unwrap();
        other
            .checkpoint(&Document::new(1, 1).unwrap(), None)
            .unwrap();
        let other_path = other.path().to_path_buf();
        let (mut host, mut e) = loaded(root.path());
        host.restore(&mut e);
        // Keep metadata invalid until the background attempt has failed.
        let mut invalid = e.document().clone();
        invalid.set_layer("x".repeat(4 * 1024 * 1024), true, 255);
        // Same recovery epoch; a host can replace the draft before restoration too.
        e.restore_recovered(invalid);
        host.epoch = e.project_epoch();
        host.backup.as_mut().unwrap().0 = e.project_epoch();
        host.clear_pending = false;
        host.next = Instant::now();
        host.poll(&mut e);
        pump(&mut host, &mut e, |h| {
            h.writer.as_ref().is_some_and(|w| !w.busy)
        });
        assert!(prior.exists());
        assert!(other_path.exists());
        assert!(host.backup.is_some());
        host.close();
        pump(&mut host, &mut e, RecoveryHost::ready_to_close);
        assert!(
            !prior.exists(),
            "explicit discard can remove recovered backup"
        );
        assert!(other_path.exists());
    }
    #[test]
    fn defer_during_loading_preserves_file_and_stops_prompt_without_mutating_original() {
        let root = tempfile::tempdir().unwrap();
        let prior = abandoned(root.path());
        let mut e = ImageEditor::new(Document::new(3, 2).unwrap());
        let original = e.document().clone();
        let mut host = RecoveryHost::new(root.path(), e.project_epoch(), None);
        host.poll(&mut e);
        assert!(host.prompt_pending());
        host.defer();
        assert!(!host.prompt_pending());
        for _ in 0..5 {
            host.poll(&mut e);
        }
        assert!(prior.exists());
        assert_eq!(e.document(), &original);
        assert!(!e.has_unsaved_changes());
        host.close();
        pump(&mut host, &mut e, RecoveryHost::ready_to_close);
        assert!(prior.exists());
    }
    #[test]
    fn corrupt_copy_does_not_block_other_drafts_or_original_and_discard_is_explicit() {
        let root = tempfile::tempdir().unwrap();
        let bad = abandoned(root.path());
        std::fs::write(&bad, b"damaged").unwrap();
        let good = abandoned(root.path());
        let (mut host, mut e) = loaded(root.path());
        let original = e.document().clone();
        while host.loaded.as_ref().unwrap().draft.is_ok() {
            host.discard_current();
            pump(&mut host, &mut e, |h| h.loaded.is_some());
        }
        assert!(host.loaded.as_ref().unwrap().draft.is_err());
        assert!(bad.exists());
        assert_eq!(e.document(), &original);
        host.discard_current();
        assert!(!bad.exists());
        // Any other draft remains untouched unless it was explicitly discarded above.
        host.defer();
        host.close();
        pump(&mut host, &mut e, RecoveryHost::ready_to_close);
        assert_eq!(e.document(), &original);
        let _ = good;
    }
    #[test]
    fn unchanged_dirty_snapshot_is_not_rewritten_and_close_drains_inflight_write() {
        let root = tempfile::tempdir().unwrap();
        let mut e = ImageEditor::new(Document::new(3, 2).unwrap());
        e.restore_recovered(draft());
        let mut host = RecoveryHost::new(root.path(), e.project_epoch(), None);
        host.interval = Duration::from_secs(3600);
        host.next = Instant::now();
        pump(&mut host, &mut e, |h| {
            h.writer.as_ref().is_some_and(|w| !w.busy && w.has_file)
        });
        let paths = snapshots(root.path());
        assert_eq!(paths.len(), 1);
        let modified = std::fs::metadata(&paths[0]).unwrap().modified().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        host.next = Instant::now();
        host.poll(&mut e);
        pump(&mut host, &mut e, |h| {
            h.writer.as_ref().is_some_and(|w| !w.busy)
        });
        assert_eq!(
            std::fs::metadata(&paths[0]).unwrap().modified().unwrap(),
            modified
        );
        host.next = Instant::now();
        host.poll(&mut e);
        assert!(host.writer.as_ref().unwrap().busy);
        host.close();
        pump(&mut host, &mut e, RecoveryHost::ready_to_close);
        assert!(snapshots(root.path()).is_empty());
    }
    #[test]
    fn new_project_while_write_is_inflight_cannot_restore_or_leave_the_previous_draft() {
        let root = tempfile::tempdir().unwrap();
        let mut e = ImageEditor::new(Document::new(3, 2).unwrap());
        e.restore_recovered(draft());
        let mut host = RecoveryHost::new(
            root.path(),
            e.project_epoch(),
            Some(PathBuf::from("previous.psd")),
        );
        host.next = Instant::now();
        host.poll(&mut e);
        assert!(host.writer.as_ref().unwrap().busy);
        e.replace(Document::new(4, 5).unwrap());
        pump(&mut host, &mut e, |h| {
            h.writer.as_ref().is_some_and(|w| !w.busy && !w.has_file) && !h.clear_pending
        });
        assert!(host.source().is_none());
        assert!(snapshots(root.path()).is_empty());
        assert_eq!(e.document(), &Document::new(4, 5).unwrap());
        host.close();
        pump(&mut host, &mut e, RecoveryHost::ready_to_close);
    }
    fn frame(
        ctx: &egui::Context,
        host: &mut RecoveryHost,
        e: &mut ImageEditor,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 700.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                host.show_prompt(ui.ctx(), e);
            },
        )
    }
    fn text_position(shapes: &[egui::epaint::ClippedShape], text: &str) -> Option<egui::Pos2> {
        fn find(shape: &egui::Shape, text: &str) -> Option<egui::Pos2> {
            match shape {
                egui::Shape::Text(t) if t.galley.text() == text => {
                    Some(t.pos + t.galley.size() * 0.5)
                }
                egui::Shape::Vec(v) => v.iter().find_map(|s| find(s, text)),
                _ => None,
            }
        }
        shapes.iter().find_map(|s| find(&s.shape, text))
    }
    #[test]
    fn centered_modal_buttons_restore_discard_and_defer_with_real_pointer_events() {
        for action in ["Восстановить", "Удалить копию", "Позже"] {
            let root = tempfile::tempdir().unwrap();
            let prior = abandoned(root.path());
            let (mut host, mut e) = loaded(root.path());
            let original = e.document().clone();
            let ctx = egui::Context::default();
            frame(&ctx, &mut host, &mut e, vec![]);
            let output = frame(&ctx, &mut host, &mut e, vec![]);
            let position = text_position(&output.shapes, action).unwrap();
            assert!(
                position.x > 200.0
                    && position.x < 900.0
                    && position.y > 150.0
                    && position.y < 650.0,
                "{position:?}"
            );
            let button = |pressed| egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            frame(
                &ctx,
                &mut host,
                &mut e,
                vec![egui::Event::PointerMoved(position), button(true)],
            );
            frame(&ctx, &mut host, &mut e, vec![button(false)]);
            assert!(!host.prompt_pending(), "{action}");
            match action {
                "Восстановить" => {
                    assert_eq!(e.document(), &draft());
                    assert!(e.has_unsaved_changes());
                    assert!(prior.exists());
                }
                "Удалить копию" => {
                    assert!(!prior.exists());
                    assert_eq!(e.document(), &original);
                }
                _ => {
                    assert!(prior.exists());
                    assert_eq!(e.document(), &original);
                }
            }
            host.close();
            pump(&mut host, &mut e, RecoveryHost::ready_to_close);
        }
    }
    #[test]
    fn failed_external_shutdown_can_resume_autorecovery_for_new_edits() {
        let root = tempfile::tempdir().unwrap();
        let mut e = ImageEditor::new(Document::new(3, 2).unwrap());
        let mut host = RecoveryHost::new(root.path(), e.project_epoch(), None);
        host.interval = Duration::from_secs(3600);
        host.close();
        pump(&mut host, &mut e, RecoveryHost::ready_to_close);
        host.cancel_close();
        e.restore_recovered(draft());
        pump(&mut host, &mut e, |h| {
            h.writer.as_ref().is_some_and(|w| !w.busy && w.has_file)
        });
        assert!(!host.ready_to_close());
        assert_eq!(snapshots(root.path()).len(), 1);
        host.close();
        pump(&mut host, &mut e, RecoveryHost::ready_to_close);
    }
}

#[cfg(test)]
mod tab_tests {
    use super::*;
    fn pump(
        host: &mut RecoveryHost,
        editor: &mut ImageEditor,
        ready: impl Fn(&RecoveryHost) -> bool,
    ) {
        let start = Instant::now();
        loop {
            host.poll(editor);
            if ready(host) {
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn independent_tab_writers_preserve_other_drafts_when_one_tab_is_saved_or_closed() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = ImageEditor::new(Document::new(3, 4).unwrap());
        let mut b = ImageEditor::new(Document::new(5, 6).unwrap());
        a.restore_recovered(a.document().clone());
        b.restore_recovered(b.document().clone());
        let mut first = RecoveryHost::new_tab(dir.path(), a.project_epoch(), Some("a.psd".into()));
        let mut second = RecoveryHost::new_tab(dir.path(), b.project_epoch(), Some("b.psd".into()));
        first.next = Instant::now();
        second.next = Instant::now();
        pump(&mut first, &mut a, |h| {
            h.writer.as_ref().is_some_and(|w| w.has_file && !w.busy)
        });
        pump(&mut second, &mut b, |h| {
            h.writer.as_ref().is_some_and(|w| w.has_file && !w.busy)
        });
        assert!(
            !second.prompt_pending(),
            "new tabs never rediscover startup drafts"
        );
        a.mark_saved();
        first.next = Instant::now();
        pump(&mut first, &mut a, |h| {
            h.writer.as_ref().is_some_and(|w| !w.has_file && !w.busy)
        });
        assert!(second.writer.as_ref().unwrap().has_file);
        first.close();
        pump(&mut first, &mut a, RecoveryHost::ready_to_close);
        assert!(second.writer.as_ref().unwrap().has_file);
        second.close();
        pump(&mut second, &mut b, RecoveryHost::ready_to_close);
    }
    #[test]
    fn multiple_startup_copies_are_handed_to_new_tabs_without_overwriting_the_first_restore() {
        let dir = tempfile::tempdir().unwrap();
        for size in [3, 5] {
            let session = Session::new(dir.path()).unwrap();
            session
                .checkpoint(&Document::new(size, 4).unwrap(), None)
                .unwrap();
            drop(session);
        }
        let mut editor = ImageEditor::new(Document::new(1, 1).unwrap());
        let mut first = RecoveryHost::new(dir.path(), editor.project_epoch(), None);
        pump(&mut first, &mut editor, |h| h.loaded.is_some());
        first.restore(&mut editor);
        let restored = editor.document().clone();
        let mut other = ImageEditor::new(Document::new(1, 1).unwrap());
        let mut second = RecoveryHost::new_tab(dir.path(), other.project_epoch(), None);
        first.transfer_pending_to(&mut second);
        assert!(!first.prompt_pending());
        assert!(second.prompt_pending());
        pump(&mut second, &mut other, |h| h.loaded.is_some());
        second.restore(&mut other);
        assert_ne!(other.document().width(), restored.width());
        assert_eq!(editor.document(), &restored);
        first.close();
        second.close();
        pump(&mut first, &mut editor, RecoveryHost::ready_to_close);
        pump(&mut second, &mut other, RecoveryHost::ready_to_close);
    }
}
