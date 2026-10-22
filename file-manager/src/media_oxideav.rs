//! Cross-platform in-process media: `OxideAV` decoding and CPAL audio output.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
#[path = "media/audio.rs"]
mod audio;
#[path = "media/oxide.rs"]
mod oxide;
/// Media categories supported by the viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Audio without a video image.
    Audio,
    /// Video, optionally accompanied by audio.
    Video,
}
/// Recognizes common local audio/video file extensions.
pub fn kind(path: &Path) -> Option<Kind> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" | "mpg" | "mpeg" | "wmv" | "ts" => {
            Some(Kind::Video)
        }
        "mp3" | "wav" | "flac" | "ogg" | "opus" | "m4a" | "aac" | "aiff" | "aif" | "wma" => {
            Some(Kind::Audio)
        }
        _ => None,
    }
}
/// Metadata returned by the asynchronous preview loader.
#[derive(Clone, Debug)]
pub struct Info {
    /// Local source file.
    pub path: PathBuf,
    /// Duration in seconds, when present in the container.
    pub duration: f64,
    /// Decoded display size; absent for audio-only files.
    pub size: Option<[usize; 2]>,
    /// The file has an audio stream.
    pub audio: bool,
}

/// Read local media metadata without launching another program.
/// # Errors
/// Returns an `OxideAV` diagnostic for unsupported or damaged containers.
pub fn probe(path: &Path) -> Result<Info, String> {
    std::panic::catch_unwind(|| oxide::probe(path))
        .map_err(|_| "OxideAV container parser failed while reading this file".to_owned())?
        .map_err(|e| format!("OxideAV: {e}"))
}
struct Video {
    pts: f64,
    size: [usize; 2],
    rgba: Vec<u8>,
}
enum Event {
    Video(Video),
    Error(String),
    Done(f64),
}
struct Worker {
    cancel: Arc<AtomicBool>,
    muted: Arc<AtomicBool>,
    clock: Arc<Mutex<Option<Instant>>>,
    receiver: Receiver<Event>,
    thread: Option<JoinHandle<()>>,
    #[cfg(test)]
    alive: Arc<AtomicBool>,
}
impl Worker {
    fn start(info: Info, offset: f64, muted: bool, ctx: egui::Context) -> Result<Self, String> {
        let cancel = Arc::new(AtomicBool::new(false));
        let mute = Arc::new(AtomicBool::new(muted));
        let clock = Arc::new(Mutex::new(None));
        let (sender, receiver) = sync_channel(16);
        let stop = cancel.clone();
        let quiet = mute.clone();
        let time = clock.clone();
        #[cfg(test)]
        let alive = Arc::new(AtomicBool::new(true));
        #[cfg(test)]
        let running = alive.clone();
        let thread = std::thread::Builder::new()
            .name("oxideav-preview".into())
            .spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(&info, offset, &stop, &quiet, &time, &sender, &ctx)
                }));
                let error = match outcome {
                    Ok(Ok(())) => None,
                    Ok(Err(e)) => Some(e),
                    Err(_) => Some("OxideAV decoder panicked while reading this file".into()),
                };
                if let Some(error) = error {
                    send(&sender, Event::Error(error), &stop, &ctx);
                }
                #[cfg(test)]
                running.store(false, Ordering::Release);
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            cancel,
            muted: mute,
            clock,
            receiver,
            thread: Some(thread),
            #[cfg(test)]
            alive,
        })
    }
    fn elapsed(&self) -> f64 {
        self.clock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .map_or(0.0, |t| t.elapsed().as_secs_f64())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn send(
    sender: &SyncSender<Event>,
    mut event: Event,
    cancel: &AtomicBool,
    ctx: &egui::Context,
) -> bool {
    loop {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        match sender.try_send(event) {
            Ok(()) => {
                ctx.request_repaint();
                return true;
            }
            Err(TrySendError::Disconnected(_)) => return false,
            Err(TrySendError::Full(e)) => event = e,
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn run(
    info: &Info,
    offset: f64,
    cancel: &AtomicBool,
    muted: &Arc<AtomicBool>,
    clock: &Mutex<Option<Instant>>,
    sender: &SyncSender<Event>,
    ctx: &egui::Context,
) -> Result<(), String> {
    let mut source = oxide::Source::open(&info.path)?;
    if offset > 0.0 {
        source.seek(offset)?;
    }
    let mut sink = None;
    let mut end = offset;
    while !cancel.load(Ordering::Relaxed) {
        let Some(frame) = source.next()? else {
            break;
        };
        let pts = frame.pts();
        if !pts.is_finite() {
            return Err("Non-finite media timestamp".into());
        }
        let audio_end = match &frame {
            oxide::Decoded::Audio {
                rate,
                channels,
                samples,
                ..
            } => pts + samples.len() as f64 / (*channels as f64 * f64::from(*rate)),
            oxide::Decoded::Video { .. } => pts + 1.0 / 30.0,
        };
        if audio_end <= offset {
            continue;
        }
        if clock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_none()
        {
            if info.audio && !muted.load(Ordering::Relaxed) {
                sink = Some(audio::Sink::open(muted.clone())?);
            }
            *clock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Instant::now());
        }
        let origin = clock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .unwrap();
        while pts - offset > origin.elapsed().as_secs_f64() + 0.15
            && !cancel.load(Ordering::Relaxed)
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        if sink.is_none() && info.audio && !muted.load(Ordering::Relaxed) {
            sink = Some(audio::Sink::open(muted.clone())?);
        }
        match frame {
            oxide::Decoded::Video { pts, size, rgba } => {
                if pts >= offset
                    && !send(sender, Event::Video(Video { pts, size, rgba }), cancel, ctx)
                {
                    return Ok(());
                }
            }
            oxide::Decoded::Audio {
                pts,
                rate,
                channels,
                samples,
            } => {
                if let Some(sink) = &mut sink {
                    let skip = (((offset - pts).max(0.0) * f64::from(rate)).round() as usize)
                        .min(samples.len() / channels)
                        * channels;
                    sink.feed(&samples[skip..], rate, channels, cancel)?;
                }
            }
        }
        end = end.max(audio_end);
        if let Some(sink) = &sink {
            sink.check()?;
        }
    }
    if let Some(origin) = *clock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
    {
        while !cancel.load(Ordering::Relaxed)
            && (origin.elapsed().as_secs_f64() < end - offset
                || sink.as_ref().is_some_and(audio::Sink::pending))
        {
            if let Some(sink) = &sink {
                sink.check()?;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    if !cancel.load(Ordering::Relaxed) {
        send(sender, Event::Done(end), cancel, ctx);
    }
    Ok(())
}
/// A preview owns its worker and device stream; replacing it synchronously stops both.
pub(crate) struct Player {
    info: Info,
    worker: Option<Worker>,
    pending: VecDeque<Video>,
    texture: Option<egui::TextureHandle>,
    offset: f64,
    muted: bool,
    autoplay_attempted: bool,
    error: Option<String>,
    done: Option<f64>,
}
impl Player {
    pub(crate) fn new(info: Info) -> Self {
        Self {
            info,
            worker: None,
            pending: VecDeque::new(),
            texture: None,
            offset: 0.0,
            muted: false,
            autoplay_attempted: false,
            error: None,
            done: None,
        }
    }
    #[cfg(test)]
    pub(crate) fn worker_alive(&self) -> Option<Arc<AtomicBool>> {
        self.worker.as_ref().map(|w| w.alive.clone())
    }
    fn position(&self) -> f64 {
        let p = self.offset + self.worker.as_ref().map_or(0.0, Worker::elapsed);
        if self.info.duration > 0.0 {
            p.min(self.info.duration)
        } else {
            p
        }
    }
    pub(crate) fn pause(&mut self) {
        self.offset = self.position();
        self.worker = None;
        self.pending.clear();
        self.done = None;
    }
    fn play(&mut self, ctx: &egui::Context) -> Result<(), String> {
        self.pause();
        self.error = None;
        if self.info.duration > 0.0 && self.offset >= self.info.duration {
            self.offset = 0.0;
        }
        self.worker = Some(Worker::start(
            self.info.clone(),
            self.offset,
            self.muted,
            ctx.clone(),
        )?);
        Ok(())
    }
    fn poll(&mut self, ctx: &egui::Context) {
        let mut failed = None;
        if let Some(worker) = &self.worker {
            while self.pending.len() < 16 {
                match worker.receiver.try_recv() {
                    Ok(Event::Video(frame)) => self.pending.push_back(frame),
                    Ok(Event::Error(error)) => {
                        failed = Some(error);
                        break;
                    }
                    Ok(Event::Done(end)) => self.done = Some(end),
                    Err(_) => break,
                }
            }
        }
        let position = self.position();
        let mut latest = None;
        while self
            .pending
            .front()
            .is_some_and(|f| f.pts <= position + 0.01)
        {
            latest = self.pending.pop_front();
        }
        if let Some(frame) = latest {
            let image = egui::ColorImage::from_rgba_unmultiplied(frame.size, &frame.rgba);
            if let Some(texture) = &mut self.texture {
                texture.set(image, egui::TextureOptions::LINEAR);
            } else {
                self.texture =
                    Some(ctx.load_texture("oxideav-frame", image, egui::TextureOptions::LINEAR));
            }
        }
        if let Some(error) = failed {
            self.pause();
            self.error = Some(error);
        } else if self.done.is_some() && self.pending.is_empty() {
            let end = self.done.unwrap();
            self.pause();
            self.offset = end;
            if self.info.duration <= 0.0 {
                self.info.duration = end;
            }
        }
    }
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, locale: &crate::Locale, autoplay: bool) {
        if autoplay && !self.autoplay_attempted {
            self.autoplay_attempted = true;
            if let Err(error) = self.play(ui.ctx()) {
                self.pause();
                self.error = Some(error);
            }
        }
        self.poll(ui.ctx());
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(locale.text(if self.worker.is_some() {
                    "Pause"
                } else {
                    "Play"
                }))
                .clicked()
            {
                if self.worker.is_some() {
                    self.pause();
                } else if let Err(error) = self.play(ui.ctx()) {
                    self.error = Some(error);
                }
            }
            if ui.button(locale.text("Stop")).clicked() {
                self.pause();
                self.offset = 0.0;
            }
            let mut position = self.position();
            if self.info.duration > 0.0 {
                let response = ui.add(
                    egui::Slider::new(&mut position, 0.0..=self.info.duration).show_value(false),
                );
                if response.drag_stopped() || (response.changed() && !response.dragged()) {
                    let playing = self.worker.is_some();
                    self.pause();
                    self.offset = position;
                    if playing {
                        if let Err(error) = self.play(ui.ctx()) {
                            self.error = Some(error);
                        }
                    }
                }
            }
            ui.label(format!(
                "{} / {}",
                timestamp(position),
                timestamp(self.info.duration)
            ));
            if self.info.audio && ui.checkbox(&mut self.muted, locale.text("Mute")).changed() {
                if let Some(worker) = &self.worker {
                    worker.muted.store(self.muted, Ordering::Relaxed);
                }
            }
        });
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if let Some(texture) = &self.texture {
            let available = ui.available_size().max(egui::vec2(1.0, 1.0));
            let size = texture.size_vec2();
            let scale = (available.x / size.x).min(available.y / size.y);
            ui.centered_and_justified(|ui| {
                ui.image((texture.id(), size * scale));
            });
        } else if self.info.size.is_none() {
            ui.centered_and_justified(|ui| {
                ui.heading(locale.text("Audio"));
            });
        }
        if self.worker.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(30));
        }
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        self.pause();
    }
}
fn timestamp(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

#[cfg(test)]
pub(crate) fn silent_wav(path: &Path) {
    oxide::silent_wav(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn poll_until(player: &mut Player, ctx: &egui::Context, condition: impl Fn(&Player) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            player.poll(ctx);
            assert!(player.error.is_none(), "{:?}", player.error);
            if condition(player) {
                return;
            }
            assert!(Instant::now() < deadline, "playback timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn play_seek_pause_stop_and_drop_join_decoder() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/media/h264-aac.mp4");
        let ctx = egui::Context::default();
        let mut player = Player::new(probe(&path).unwrap());
        player.muted = true;
        player.play(&ctx).unwrap();
        let alive = player.worker.as_ref().unwrap().alive.clone();
        poll_until(&mut player, &ctx, |p| {
            p.texture.is_some() && p.position() > 0.05
        });
        player.pause();
        assert!(!alive.load(Ordering::Acquire));
        let offset = player.position();
        std::thread::sleep(Duration::from_millis(20));
        assert!((offset - player.position()).abs() < f64::EPSILON);
        player.offset = 0.5;
        player.play(&ctx).unwrap();
        poll_until(&mut player, &ctx, |p| {
            p.position() > 0.55 && p.texture.is_some()
        });
        let alive = player.worker.as_ref().unwrap().alive.clone();
        drop(player);
        assert!(!alive.load(Ordering::Acquire));
    }
    #[test]
    fn eof_stops_playback_and_malformed_file_reports_error() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("sound.wav");
        oxide::silent_wav(&path);
        let ctx = egui::Context::default();
        let mut player = Player::new(probe(&path).unwrap());
        player.muted = true;
        player.offset = 2.8;
        player.play(&ctx).unwrap();
        poll_until(&mut player, &ctx, |p| p.worker.is_none());
        assert!(player.offset >= 2.9);
        std::fs::write(&path, b"invalid").unwrap();
        assert!(probe(&path).is_err());
    }
    #[test]
    #[ignore = "requires a real audio output device"]
    fn cpal_output_stops_with_preview() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("sound.wav");
        oxide::silent_wav(&path);
        let ctx = egui::Context::default();
        let mut player = Player::new(probe(&path).unwrap());
        player.play(&ctx).unwrap();
        let alive = player.worker.as_ref().unwrap().alive.clone();
        poll_until(&mut player, &ctx, |p| p.position() > 0.2);
        player.pause();
        assert!(!alive.load(Ordering::Acquire));
    }
}
