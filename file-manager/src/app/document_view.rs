//! Asynchronous page navigation shared by the quick and full viewer.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
use crate::{
    archive::TempFolder,
    documents::{Cancellation, Page, Provider},
    Locale,
};
use std::{
    path::PathBuf,
    sync::{
        mpsc::{channel, RecvTimeoutError, Sender},
        Arc, Mutex,
    },
    time::Duration,
};
struct Request {
    generation: u64,
    page: usize,
    side: u32,
}
struct Reply {
    generation: u64,
    count: usize,
    page: usize,
    result: Result<Page, String>,
}
type Slot = Arc<Mutex<Option<Reply>>>;
pub(super) struct View {
    cancel: Cancellation,
    request: Sender<Request>,
    reply: Slot,
    page: usize,
    count: usize,
    generation: u64,
    side: u32,
    zoom: f32,
    texture: Option<egui::TextureHandle>,
    error: Option<String>,
    loading: bool,
    full: bool,
    scroll_id: egui::Id,
}
impl View {
    pub(super) fn start(
        path: PathBuf,
        provider: Arc<dyn Provider>,
        temporary: Option<Arc<TempFolder>>,
        full: bool,
        wake: impl Fn() + Send + 'static,
    ) -> Self {
        let scroll_id = egui::Id::new(("document-preview", &path, full));
        let cancel = Cancellation::default();
        let stop = cancel.clone();
        let (sender, receiver) = channel::<Request>();
        let reply = Arc::new(Mutex::new(None));
        let result = reply.clone();
        let spawned = std::thread::Builder::new()
            .name("document-preview".into())
            .stack_size(64 * 1024 * 1024)
            .spawn(move || {
                worker(
                    &path,
                    provider.as_ref(),
                    temporary,
                    &stop,
                    &receiver,
                    &result,
                    wake,
                );
            });
        let mut view = Self {
            cancel,
            request: sender,
            reply,
            page: 0,
            count: 0,
            generation: 0,
            side: 1600,
            zoom: 1.0,
            texture: None,
            error: None,
            loading: true,
            full,
            scroll_id,
        };
        if let Err(e) = spawned {
            view.error = Some(e.to_string());
            view.loading = false;
        } else {
            view.send();
        }
        view
    }
    fn send(&mut self) {
        self.loading = true;
        self.error = None;
        if self
            .request
            .send(Request {
                generation: self.generation,
                page: self.page,
                side: self.side,
            })
            .is_err()
        {
            self.loading = false;
            self.error = Some("Document preview worker stopped".into());
        }
    }
    fn poll(&mut self, ctx: &egui::Context) {
        let result = self
            .reply
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(reply) = result
            .filter(|r| r.generation == self.generation || (r.count == 0 && r.result.is_err()))
        {
            self.loading = false;
            self.count = reply.count;
            self.page = reply.page;
            match reply.result {
                Ok(page) => {
                    let valid = page.size[0]
                        .checked_mul(page.size[1])
                        .and_then(|n| n.checked_mul(4));
                    if valid != Some(page.rgba.len())
                        || page.size.contains(&0)
                        || page.size.iter().any(|s| *s > 4096)
                    {
                        self.error = Some("Invalid page bitmap".into());
                        return;
                    }
                    let image = egui::ColorImage::from_rgba_premultiplied(page.size, &page.rgba);
                    self.texture = Some(ctx.load_texture(
                        "document-preview-page",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
                Err(error) => {
                    self.texture = None;
                    self.error = Some(error);
                }
            }
        }
    }
    fn navigate(&mut self, page: usize) {
        let page = page.min(self.count.saturating_sub(1));
        if page != self.page {
            self.page = page;
            self.generation += 1;
            self.texture = None;
            self.send();
        }
    }
    pub(super) fn show(&mut self, ui: &mut egui::Ui, locale: &Locale) {
        self.poll(ui.ctx());
        let mut page = self.page;
        let mut zoom = self.zoom;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(page > 0, egui::Button::new("◀"))
                .on_hover_text(locale.text("Previous page"))
                .clicked()
            {
                page -= 1;
            }
            ui.label(locale.text("Page"));
            let mut number = page + 1;
            if ui
                .add_enabled(
                    self.count > 0,
                    egui::DragValue::new(&mut number).range(1..=self.count.max(1)),
                )
                .changed()
            {
                page = number.saturating_sub(1);
            }
            ui.label(format!("/ {}", self.count));
            if ui
                .add_enabled(page + 1 < self.count, egui::Button::new("▶"))
                .on_hover_text(locale.text("Next page"))
                .clicked()
            {
                page += 1;
            }
            ui.separator();
            if ui.small_button("−").clicked() {
                zoom /= 1.25;
            }
            if ui
                .button(format!("{:.0}%", zoom * 100.0))
                .on_hover_text(locale.text("Fit width"))
                .clicked()
            {
                zoom = 1.0;
            }
            if ui.small_button("+").clicked() {
                zoom *= 1.25;
            }
            ui.weak(locale.text("Read-only preview"));
        });
        if self.full && !ui.ctx().egui_wants_keyboard_input() {
            ui.input(|i| {
                if i.key_pressed(egui::Key::PageDown) {
                    page = page.saturating_add(1);
                }
                if i.key_pressed(egui::Key::PageUp) {
                    page = page.saturating_sub(1);
                }
            });
        }
        if ui.rect_contains_pointer(ui.max_rect()) {
            ui.input(|i| {
                if i.modifiers.ctrl {
                    zoom *= i.zoom_delta();
                }
            });
        }
        self.navigate(page);
        zoom = zoom.clamp(0.25, 4.0);
        if (zoom - self.zoom).abs() > 0.001 {
            self.zoom = zoom;
            let side = (1600.0 * zoom).clamp(800.0, 4096.0) as u32;
            if side != self.side {
                self.side = side;
                self.generation += 1;
                self.send();
            }
        }
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if self.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(locale.text("Reading…"));
            });
        }
        if let Some(texture) = &self.texture {
            let size = texture.size_vec2();
            let width = (ui.available_width() - 16.0).max(64.0) * self.zoom;
            let draw = size * (width / size.x);
            egui::ScrollArea::both()
                .id_salt((self.scroll_id, self.page))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.image((texture.id(), draw));
                });
        }
    }
}
impl Drop for View {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
fn worker(
    path: &std::path::Path,
    provider: &dyn Provider,
    temporary: Option<Arc<TempFolder>>,
    stop: &Cancellation,
    receiver: &std::sync::mpsc::Receiver<Request>,
    result: &Slot,
    wake: impl Fn(),
) {
    // Extraction must outlive asynchronous opening, layout and rasterization.
    let _temporary = temporary;
    let Some(admission) = Permit::acquire(stop) else {
        return;
    };
    let job = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let document = provider.open(path, stop);
        let mut document = match document {
            Ok(doc) => doc,
            Err(error) => {
                publish(
                    result,
                    Reply {
                        generation: 0,
                        count: 0,
                        page: 0,
                        result: Err(error),
                    },
                    &wake,
                );
                return;
            }
        };
        drop(admission); // Idle previews do not occupy a parsing/rendering slot.
        while !stop.is_cancelled() {
            let mut request = match receiver.recv_timeout(Duration::from_millis(25)) {
                Ok(r) => r,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            // Only render the latest request after fast page/zoom changes.
            while let Ok(newer) = receiver.try_recv() {
                request = newer;
            }
            if stop.is_cancelled() {
                break;
            }
            let Some(permit) = Permit::acquire(stop) else {
                break;
            };
            let count = document.page_count();
            let page = request.page.min(count.saturating_sub(1));
            let bitmap = if count == 0 {
                Err("The document contains no pages".into())
            } else {
                document.render(page, request.side)
            };
            drop(permit);
            if stop.is_cancelled() {
                break;
            }
            publish(
                result,
                Reply {
                    generation: request.generation,
                    count,
                    page,
                    result: bitmap,
                },
                &wake,
            );
        }
    }));
    if job.is_err() && !stop.is_cancelled() {
        publish(
            result,
            Reply {
                generation: 0,
                count: 0,
                page: 0,
                result: Err("Document preview failed while parsing or rendering this file".into()),
            },
            &wake,
        );
    }
}

// Bound simultaneous parsers/layouts while a cancelled library call winds down.
static ACTIVE: Mutex<usize> = Mutex::new(0);
struct Permit;
impl Permit {
    fn acquire(cancel: &Cancellation) -> Option<Self> {
        loop {
            if cancel.is_cancelled() {
                return None;
            }
            let mut active = ACTIVE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if *active < 2 {
                *active += 1;
                return Some(Self);
            }
            drop(active);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        *ACTIVE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) -= 1;
    }
}
fn publish(slot: &Slot, result: Reply, wake: &impl Fn()) {
    *slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(result);
    wake();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    struct Fake(Arc<std::sync::atomic::AtomicBool>);
    struct Pages;
    impl crate::documents::Document for Pages {
        fn page_count(&self) -> usize {
            3
        }
        fn render(&mut self, page: usize, _side: u32) -> Result<Page, String> {
            std::thread::sleep(Duration::from_millis(15));
            Ok(Page {
                size: [2, 2],
                rgba: vec![u8::try_from(page * 50).unwrap(); 16],
            })
        }
    }
    impl Provider for Fake {
        fn supports(&self, _: &std::path::Path) -> bool {
            true
        }
        fn open(
            &self,
            _: &std::path::Path,
            cancel: &Cancellation,
        ) -> Result<Box<dyn crate::documents::Document>, String> {
            let flag = self.0.clone();
            cancel.on_cancel(move || flag.store(true, std::sync::atomic::Ordering::Release));
            Ok(Box::new(Pages))
        }
    }
    fn ready(view: &mut View, ctx: &egui::Context) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while view.loading {
            view.poll(ctx);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(view.error.is_none(), "{:?}", view.error);
    }
    #[test]
    fn page_requests_ignore_old_results_and_drop_cancels() {
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ctx = egui::Context::default();
        let mut view = View::start(
            "example.docx".into(),
            Arc::new(Fake(cancelled.clone())),
            None,
            true,
            || {},
        );
        ready(&mut view, &ctx);
        assert_eq!(view.count, 3);
        view.navigate(1);
        view.navigate(2);
        *view.reply.lock().unwrap() = Some(Reply {
            generation: 0,
            count: 3,
            page: 0,
            result: Ok(Page {
                size: [1, 1],
                rgba: vec![255; 4],
            }),
        });
        view.poll(&ctx);
        assert!(view.loading);
        assert!(view.texture.is_none());
        ready(&mut view, &ctx);
        assert_eq!(view.page, 2);
        assert!(view.texture.is_some());
        drop(view);
        assert!(cancelled.load(std::sync::atomic::Ordering::Acquire));
    }
    #[test]
    fn idle_documents_do_not_block_other_previews() {
        let ctx = egui::Context::default();
        let mut views = Vec::new();
        for index in 0..3 {
            let mut view = View::start(
                format!("file-{index}.docx").into(),
                Arc::new(Fake(Arc::new(std::sync::atomic::AtomicBool::new(false)))),
                None,
                true,
                || {},
            );
            ready(&mut view, &ctx);
            views.push(view);
        }
        assert_eq!(views.len(), 3);
    }
    #[test]
    fn extraction_survives_until_cancelled_worker_finishes() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("a.docx");
        std::fs::write(&source, "fixture").unwrap();
        let zip_path = root.path().join("documents.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        zip.start_file("a.docx", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"fixture").unwrap();
        zip.finish().unwrap();
        let (extracted, folder) =
            crate::archive::extract_to_temporary(&zip_path, "a.docx").unwrap();
        let folder = Arc::new(folder);
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut view = View::start(
            extracted.clone(),
            Arc::new(Fake(cancelled)),
            Some(folder.clone()),
            false,
            || {},
        );
        ready(&mut view, &egui::Context::default());
        drop(folder);
        assert!(extracted.exists());
        drop(view);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while extracted.exists() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
