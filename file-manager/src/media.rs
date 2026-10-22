//! In-panel video decoding through local `FFmpeg`; sound uses Windows `MediaPlayer`.
//! On other platforms sound falls back to `FFplay`. Decoders belong to the preview.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{sync_channel, Receiver},
    time::{Duration, Instant},
};

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

fn command(tool: &str) -> Command {
    let name = format!("MUFOED_{}", tool.to_ascii_uppercase());
    let mut command = Command::new(std::env::var_os(name).unwrap_or_else(|| tool.into()));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    command
}
/// Inspects a local media file on the preview worker.
/// # Errors
/// Returns a readable diagnostic when `FFprobe` is missing or cannot read the file.
pub fn probe(path: &Path) -> Result<Info, String> {
    #[cfg(windows)]
    if kind(path) == Some(Kind::Audio) {
        return Ok(Info {
            path: path.into(),
            duration: 0.0,
            size: None,
            audio: true,
        });
    }
    let output = command("ffprobe")
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(path)
        .stdout(Stdio::piped())
        .output()
        .map_err(|e| format!("FFprobe: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    info(path, &json)
}
fn info(path: &Path, json: &serde_json::Value) -> Result<Info, String> {
    let streams = json["streams"].as_array().ok_or("No media streams")?;
    let video = streams
        .iter()
        .find(|s| s["codec_type"] == "video" && s["disposition"]["attached_pic"] != 1);
    let audio = streams.iter().any(|s| s["codec_type"] == "audio");
    if video.is_none() && !audio {
        return Err("No playable audio or video stream".into());
    }
    let duration = json["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| {
            streams
                .iter()
                .filter_map(|s| s["duration"].as_str()?.parse::<f64>().ok())
                .reduce(f64::max)
        })
        .filter(|n| n.is_finite() && *n > 0.0)
        .unwrap_or(0.0);
    let size = video.map(|s| {
        let mut width = s["width"].as_u64().unwrap_or(1).clamp(1, 65536) as f64;
        let mut height = s["height"].as_u64().unwrap_or(1).clamp(1, 65536) as f64;
        let rotation = s["side_data_list"]
            .as_array()
            .and_then(|a| a.iter().find_map(|d| d["rotation"].as_f64()))
            .unwrap_or(0.0);
        if rotation.rem_euclid(180.0) > 45.0 && rotation.rem_euclid(180.0) < 135.0 {
            std::mem::swap(&mut width, &mut height);
        }
        let scale = (1280.0 / width).min(720.0 / height).min(1.0);
        [
            (width * scale).round().max(1.0) as usize,
            (height * scale).round().max(1.0) as usize,
        ]
    });
    Ok(Info {
        path: path.into(),
        duration,
        size,
        audio,
    })
}

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
/// The active preview owns its decoders and texture.
pub(crate) struct Player {
    info: Info,
    #[cfg(windows)]
    native: Option<crate::native_audio::Audio>,
    processes: Vec<Process>,
    reader: Option<std::thread::JoinHandle<()>>,
    frames: Option<Receiver<Vec<u8>>>,
    texture: Option<egui::TextureHandle>,
    offset: f64,
    started: Option<Instant>,
    muted: bool,
    autoplay_attempted: bool,
    error: Option<String>,
}
impl Player {
    pub(crate) fn new(info: Info) -> Self {
        Self {
            info,
            #[cfg(windows)]
            native: None,
            processes: Vec::new(),
            reader: None,
            frames: None,
            texture: None,
            offset: 0.0,
            started: None,
            muted: false,
            autoplay_attempted: false,
            error: None,
        }
    }
    #[cfg(all(test, windows))]
    pub(crate) fn native_handle(&self) -> Option<windows::Media::Playback::MediaPlayer> {
        self.native.as_ref().map(crate::native_audio::Audio::handle)
    }
    fn position(&self) -> f64 {
        #[cfg(windows)]
        if self.started.is_some() {
            if let Some(position) = self
                .native
                .as_ref()
                .and_then(crate::native_audio::Audio::position)
            {
                return position;
            }
        }
        let position = self.offset + self.started.map_or(0.0, |at| at.elapsed().as_secs_f64());
        if self.info.duration > 0.0 {
            position.min(self.info.duration)
        } else {
            position
        }
    }
    pub(crate) fn pause(&mut self) {
        self.offset = self.position();
        self.started = None;
        #[cfg(windows)]
        if let Some(native) = &self.native {
            native.pause();
        }
        self.processes.clear();
        self.frames = None;
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
    fn play(&mut self, ctx: &egui::Context) -> Result<(), String> {
        self.pause();
        self.error = None;
        if self.info.duration > 0.0 && self.offset >= self.info.duration {
            self.offset = 0.0;
        }
        let seek = self.offset.to_string();
        if let Some([width, height]) = self.info.size {
            let filter = format!("fps=30,scale={width}:{height}");
            let mut child = Process(
                command("ffmpeg")
                    .args([
                        "-nostdin",
                        "-v",
                        "error",
                        "-threads",
                        "2",
                        "-re",
                        "-ss",
                        &seek,
                        "-protocol_whitelist",
                        "file,pipe",
                        "-i",
                    ])
                    .arg(&self.info.path)
                    .args([
                        "-map", "0:V:0", "-an", "-sn", "-vf", &filter, "-pix_fmt", "rgba", "-f",
                        "rawvideo", "pipe:1",
                    ])
                    .stdout(Stdio::piped())
                    .spawn()
                    .map_err(|e| format!("FFmpeg: {e}"))?,
            );
            let mut output = child.0.stdout.take().ok_or("FFmpeg output unavailable")?;
            let (sender, receiver) = sync_channel(2);
            let wake = ctx.clone();
            self.processes.push(child);
            self.reader = Some(
                std::thread::Builder::new()
                    .name("file-manager-video".into())
                    .spawn(move || loop {
                        let mut pixels = vec![0; width * height * 4];
                        if output.read_exact(&mut pixels).is_err() {
                            break;
                        }
                        match sender.try_send(pixels) {
                            Ok(()) => wake.request_repaint(),
                            Err(std::sync::mpsc::TrySendError::Full(_)) => {}
                            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => break,
                        }
                    })
                    .map_err(|e| e.to_string())?,
            );
            self.frames = Some(receiver);
        }
        #[cfg(windows)]
        if self.info.audio {
            if self.native.is_none() {
                self.native = Some(
                    crate::native_audio::Audio::new(&self.info.path, self.muted)
                        .map_err(|e| e.to_string())?,
                );
            }
            self.native
                .as_mut()
                .expect("native audio")
                .play(self.offset, self.muted)
                .map_err(|e| e.to_string())?;
        }
        #[cfg(not(windows))]
        if self.info.audio && !self.muted {
            match command("ffplay")
                .args([
                    "-v",
                    "error",
                    "-nodisp",
                    "-autoexit",
                    "-vn",
                    "-ss",
                    &seek,
                    "-protocol_whitelist",
                    "file,pipe",
                ])
                .arg(&self.info.path)
                .spawn()
            {
                Ok(child) => self.processes.push(Process(child)),
                Err(error) => {
                    self.pause();
                    return Err(format!("FFplay: {error}"));
                }
            }
        }
        self.started = Some(Instant::now());
        Ok(())
    }
    fn poll(&mut self, ctx: &egui::Context) {
        #[cfg(windows)]
        if let Some(native) = &mut self.native {
            match native.poll() {
                Ok(duration) if duration > 0.0 => self.info.duration = duration,
                Ok(_) => {}
                Err(error) => {
                    self.pause();
                    self.error = Some(error);
                }
            }
        }
        let mut failed = None;
        let mut finished = !self.processes.is_empty();
        for process in &mut self.processes {
            let status = process.0.try_wait();
            finished &= matches!(status, Ok(Some(_)));
            if let Ok(Some(status)) = status {
                if !status.success() {
                    let mut message = String::new();
                    if let Some(mut stderr) = process.0.stderr.take() {
                        let _ = stderr.read_to_string(&mut message);
                    }
                    failed = Some(if message.is_empty() {
                        status.to_string()
                    } else {
                        message
                    });
                }
            }
        }
        if let Some(error) = failed {
            self.pause();
            self.error = Some(error);
        }
        if let (Some(receiver), Some(size)) = (&self.frames, self.info.size) {
            let mut latest = None;
            while let Ok(pixels) = receiver.try_recv() {
                latest = Some(pixels);
            }
            if let Some(pixels) = latest {
                let image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
                if let Some(texture) = &mut self.texture {
                    texture.set(image, egui::TextureOptions::LINEAR);
                } else {
                    self.texture =
                        Some(ctx.load_texture("media-frame", image, egui::TextureOptions::LINEAR));
                }
            }
        }
        if finished || (self.info.duration > 0.0 && self.position() >= self.info.duration) {
            self.pause();
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
                .button(locale.text(if self.started.is_some() {
                    "Pause"
                } else {
                    "Play"
                }))
                .clicked()
            {
                if self.started.is_some() {
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
                    let playing = self.started.is_some();
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
            if self.info.audio
                && ui.checkbox(&mut self.muted, locale.text("Mute")).changed()
                && self.started.is_some()
            {
                if let Err(error) = self.play(ui.ctx()) {
                    self.error = Some(error);
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
        if self.started.is_some() {
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
mod tests {
    use super::*;
    #[test]
    fn probe_distinguishes_cover_art_from_video_and_bounds_frames() {
        let json = serde_json::json!({"format":{"duration":"5.5"},"streams":[
            {"codec_type":"video","width":5000,"height":5000,"disposition":{"attached_pic":1}},
            {"codec_type":"audio"}]});
        let audio = info(Path::new("song.mp3"), &json).unwrap();
        assert!(audio.audio);
        assert!(audio.size.is_none());
        assert!((audio.duration - 5.5).abs() < f64::EPSILON);
        let json = serde_json::json!({"streams":[{"codec_type":"video","width":3840,"height":2160,"side_data_list":[{"rotation":90}]}]});
        assert_eq!(
            info(Path::new("clip.mp4"), &json).unwrap().size,
            Some([405, 720])
        );
    }
}

#[cfg(test)]
mod playback_tests {
    use super::*;
    #[test]
    #[ignore = "requires FFmpeg and an audio output (Windows MediaPlayer; FFplay on other platforms)"]
    fn real_video_audio_seek_pause_and_decoder_cleanup() {
        let directory = tempfile::tempdir().unwrap();
        let video = directory.path().join("clip with spaces.mp4");
        let result = command("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=160x90:rate=15",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=44100",
                "-t",
                "3",
                "-c:v",
                "mpeg4",
                "-c:a",
                "aac",
                "-y",
            ])
            .arg(&video)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let info = probe(&video).unwrap();
        assert_eq!(info.size, Some([160, 90]));
        assert!(info.audio);
        let ctx = egui::Context::default();
        let mut player = Player::new(info);
        player.muted = true;
        player.play(&ctx).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while player.texture.is_none() || player.position() <= 0.0 {
            player.poll(&ctx);
            assert!(player.error.is_none(), "{:?}", player.error);
            assert!(Instant::now() < deadline, "video frame timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(player.position() > 0.0);
        player.pause();
        assert!(player.processes.is_empty() && player.reader.is_none() && player.started.is_none());
        player.offset = 1.0;
        player.play(&ctx).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while player.position() < 0.9 {
            player.poll(&ctx);
            assert!(player.error.is_none(), "{:?}", player.error);
            assert!(Instant::now() < deadline, "seek timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
        player.pause();
        let audio = directory.path().join("sound.wav");
        let result = command("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(&video)
            .args(["-vn", "-c:a", "pcm_s16le", "-y"])
            .arg(&audio)
            .output()
            .unwrap();
        assert!(result.status.success());
        let info = probe(&audio).unwrap();
        assert!(info.size.is_none() && info.audio);
        let mut player = Player::new(info);
        player.muted = true;
        player.play(&ctx).unwrap();
        std::thread::sleep(Duration::from_millis(250));
        player.poll(&ctx);
        assert!(player.error.is_none(), "{:?}", player.error);
        #[cfg(windows)]
        assert!(player.native.is_some() && player.processes.is_empty());
        #[cfg(not(windows))]
        assert!(player.processes[0].0.try_wait().unwrap().is_none());
        player.pause();
        assert!(player.processes.is_empty());
        assert!(matches!(
            crate::preview::load(&audio, 512),
            crate::preview::Content::Media(_)
        ));
    }
}
