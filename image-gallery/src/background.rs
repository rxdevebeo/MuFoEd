//! Background removal with `BiRefNet`, the model Image Studio uses.
//!
//! The pixels are prepared here: transparency composed onto white, the
//! picture resized to the model's input and normalized; the mask comes back
//! as logits, is resized to the picture and multiplied into its alpha, so
//! the result keeps the source's bit depth and full resolution. Only the
//! ONNX graph runs outside, in a Python worker (`numpy` and `onnxruntime`)
//! that loads the model once and then serves every picture of a batch.
//!
//! The weights are the official `BiRefNet` release assets, with the same
//! download receipt (`<model>.json`, SHA-256) as Image Studio's: a model
//! fetched by either tool is accepted by the other.

// Tensor sizes are bounded by the model input (at most 4096 a side).
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use image::imageops::FilterType;
use image::{ColorType, DynamicImage, GrayImage, Luma};
use serde::{Deserialize, Serialize};

use crate::ops::Operation;
use crate::Error;

/// The ID of the background remover in [`crate::ops::Tools`].
pub const TOOL_ID: &str = "remove-background";

/// The worker script, written to a temporary folder when first needed.
const SCRIPT: &str = include_str!("../scripts/birefnet_worker.py");

/// How long loading the model may take.
const START_TIMEOUT: Duration = Duration::from_secs(600);
/// How long one picture may take.
const RUN_TIMEOUT: Duration = Duration::from_secs(900);

/// `ImageNet` normalization, as `BiRefNet` was trained.
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

/// The published `BiRefNet` weights.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Variant {
    /// General use, Swin-Tiny backbone: 214 MiB, the fastest.
    #[default]
    Tiny,
    /// General use, full size: 928 MiB.
    General,
    /// Soft edges (hair, fur): 928 MiB.
    Matting,
}

impl Variant {
    /// Every variant, for menus.
    pub const ALL: [Self; 3] = [Self::Tiny, Self::General, Self::Matting];

    /// The worker's name for it, also the default file name.
    pub fn id(self) -> &'static str {
        match self {
            Self::Tiny => "tiny",
            Self::General => "general",
            Self::Matting => "matting",
        }
    }

    /// A label with the download size.
    pub fn label(self) -> &'static str {
        match self {
            Self::Tiny => "General Tiny · 214 MiB",
            Self::General => "General · 928 MiB",
            Self::Matting => "Matting · 928 MiB",
        }
    }
}

/// Where the model and Python are, and how to run it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The ONNX weights; `None` means the variant's file in [`models_folder`].
    pub model: Option<PathBuf>,
    /// The variant to download or to look for by default.
    pub variant: Variant,
    /// Use CUDA through onnxruntime when it is available.
    pub gpu: bool,
    /// The Python interpreter; `None` means [`python`].
    pub python: Option<PathBuf>,
}

impl Settings {
    /// The model file used.
    pub fn model_path(&self) -> PathBuf {
        self.model
            .clone()
            .unwrap_or_else(|| default_model(self.variant))
    }

    /// The interpreter used.
    pub fn python_path(&self) -> PathBuf {
        self.python.clone().unwrap_or_else(python)
    }
}

/// The folder shared models live in: `%LOCALAPPDATA%\MuFoEd\models\BiRefNet`
/// on Windows, `$XDG_DATA_HOME/mufoed/models/BiRefNet` (or
/// `~/.local/share/...`) elsewhere. `MUFOED_MODELS` overrides the
/// `models` folder.
pub fn models_folder() -> PathBuf {
    let models = std::env::var_os("MUFOED_MODELS").map_or_else(
        || {
            let base = if cfg!(windows) {
                std::env::var_os("LOCALAPPDATA").map(|path| PathBuf::from(path).join("MuFoEd"))
            } else {
                std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .or_else(|| {
                        std::env::var_os("HOME")
                            .map(|home| PathBuf::from(home).join(".local/share"))
                    })
                    .map(|path| path.join("mufoed"))
            };
            base.unwrap_or_else(std::env::temp_dir).join("models")
        },
        PathBuf::from,
    );
    models.join("BiRefNet")
}

/// The default file of a variant.
pub fn default_model(variant: Variant) -> PathBuf {
    models_folder().join(format!("{}.onnx", variant.id()))
}

/// The Python interpreter: `IMAGE_GALLERY_PYTHON`, then Image Studio's
/// `IMAGE_STUDIO_PYTHON`, then a per-user Python 3.11 on Windows, then
/// `python` (Windows) or `python3` from the `PATH`.
pub fn python() -> PathBuf {
    for variable in ["IMAGE_GALLERY_PYTHON", "IMAGE_STUDIO_PYTHON"] {
        if let Some(path) = std::env::var_os(variable) {
            return PathBuf::from(path);
        }
    }
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let installed = PathBuf::from(local).join("Programs/Python/Python311/python.exe");
            if installed.is_file() {
                return installed;
            }
        }
        PathBuf::from("python")
    } else {
        PathBuf::from("python3")
    }
}

/// Keeps the temporary folders of concurrent workers apart.
static FOLDERS: AtomicU64 = AtomicU64::new(0);

/// A temporary folder holding the worker script, its log and the tensors.
struct Scratch(PathBuf);

impl Scratch {
    fn new(script: &str) -> Result<Self, Error> {
        let path = std::env::temp_dir().join(format!(
            "image-gallery-birefnet-{}-{}",
            std::process::id(),
            FOLDERS.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).map_err(|error| Error::io(&path, error))?;
        let scratch = Self(path);
        let file = scratch.script();
        std::fs::write(&file, script).map_err(|error| Error::io(&file, error))?;
        Ok(scratch)
    }

    fn script(&self) -> PathBuf {
        self.0.join("birefnet_worker.py")
    }

    fn log(&self) -> PathBuf {
        self.0.join("worker.log")
    }

    /// The end of the worker's error output, for messages.
    fn log_tail(&self) -> String {
        let text = std::fs::read_to_string(self.log()).unwrap_or_default();
        let lines: Vec<&str> = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();
        lines[lines.len().saturating_sub(6)..].join("\n")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Starts `script` with `mode` and its arguments; stdout lines arrive on the
/// returned channel, stderr goes to the scratch log.
fn spawn(
    python: &Path,
    scratch: &Scratch,
    arguments: &[&OsStr],
) -> Result<(Child, Receiver<String>), Error> {
    let log =
        std::fs::File::create(scratch.log()).map_err(|error| Error::io(scratch.log(), error))?;
    let mut command = Command::new(python);
    command
        .arg(scratch.script())
        .args(arguments)
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flashes up behind the interface.
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|error| {
        Error::Invalid(format!(
            "Python could not be started ({}): {error}",
            python.display()
        ))
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Invalid("the worker has no output".into()))?;
    let (sender, lines) = channel();
    let _ = std::thread::Builder::new()
        .name("image-gallery-birefnet-output".into())
        .spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else {
                    break;
                };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
    Ok((child, lines))
}

/// Reads the next JSON message, failing on timeout, on the worker's exit or
/// on an `error` message.
fn message(
    lines: &Receiver<String>,
    timeout: Duration,
    scratch: &Scratch,
) -> Result<serde_json::Value, Error> {
    let line = match lines.recv_timeout(timeout) {
        Ok(line) => line,
        Err(RecvTimeoutError::Timeout) => {
            return Err(Error::Invalid(
                "the background remover did not answer in time".into(),
            ));
        }
        Err(RecvTimeoutError::Disconnected) => {
            let tail = scratch.log_tail();
            return Err(Error::Invalid(if tail.is_empty() {
                "the background remover stopped".into()
            } else {
                format!("the background remover stopped:\n{tail}")
            }));
        }
    };
    let value: serde_json::Value = serde_json::from_str(&line)
        .map_err(|_| Error::Invalid(format!("unexpected worker output: {line}")))?;
    if let Some(error) = value.get("error").and_then(serde_json::Value::as_str) {
        return Err(Error::Invalid(format!("background removal: {error}")));
    }
    Ok(value)
}

/// A running worker with the model loaded.
struct Worker {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    width: u32,
    height: u32,
    /// What it was started with, to notice a change of settings.
    settings: Settings,
    calls: u64,
    scratch: Scratch,
}

impl Worker {
    fn start(settings: &Settings, script: &str) -> Result<Self, Error> {
        let model = settings.model_path();
        if !model.is_file() {
            return Err(Error::Invalid(format!(
                "the BiRefNet model is not found: {}",
                model.display()
            )));
        }
        let scratch = Scratch::new(script)?;
        let device = if settings.gpu { "auto" } else { "cpu" };
        let (mut child, lines) = spawn(
            &settings.python_path(),
            &scratch,
            &[
                OsStr::new("serve"),
                OsStr::new("--model"),
                model.as_os_str(),
                OsStr::new("--device"),
                OsStr::new(device),
            ],
        )?;
        let ready = message(&lines, START_TIMEOUT, &scratch);
        let size = ready.and_then(|ready| {
            let side = |key: &str| {
                ready
                    .get(key)
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
                    .filter(|value| (64..=4096).contains(value))
            };
            side("width")
                .zip(side("height"))
                .ok_or_else(|| Error::Invalid("the worker reported no input size".into()))
        });
        let (width, height) = match size {
            Ok(size) => size,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Invalid("the worker has no input".into()))?;
        Ok(Self {
            child,
            stdin,
            lines,
            width,
            height,
            settings: settings.clone(),
            calls: 0,
            scratch,
        })
    }

    /// Runs the graph on one input tensor; returns the logits.
    fn run(&mut self, tensor: &[f32]) -> Result<Vec<f32>, Error> {
        self.calls += 1;
        let input = self.scratch.0.join(format!("input-{}.bin", self.calls));
        let output = self.scratch.0.join(format!("output-{}.bin", self.calls));
        let bytes: Vec<u8> = tensor
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        std::fs::write(&input, bytes).map_err(|error| Error::io(&input, error))?;
        let request = serde_json::json!({
            "input": input.to_string_lossy(),
            "output": output.to_string_lossy(),
        });
        writeln!(self.stdin, "{request}")
            .and_then(|()| self.stdin.flush())
            .map_err(|error| Error::Invalid(format!("the background remover stopped: {error}")))?;
        let answer = message(&self.lines, RUN_TIMEOUT, &self.scratch);
        let _ = std::fs::remove_file(&input);
        answer?;
        let data = std::fs::read(&output).map_err(|error| Error::io(&output, error))?;
        let _ = std::fs::remove_file(&output);
        let expected = self.width as usize * self.height as usize;
        if data.len() != expected * 4 {
            return Err(Error::Invalid("the mask has the wrong size".into()));
        }
        Ok(data
            .chunks_exact(4)
            .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            .collect())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The background remover: settings and the worker, shared by every job.
pub struct Remover {
    settings: Mutex<Settings>,
    worker: Mutex<Option<Worker>>,
    script: &'static str,
}

impl std::fmt::Debug for Remover {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Remover")
            .field(
                "settings",
                &*self.settings.lock().unwrap_or_else(PoisonError::into_inner),
            )
            .finish_non_exhaustive()
    }
}

impl Remover {
    /// A remover; nothing starts until the first picture.
    pub fn new(settings: Settings) -> Self {
        Self::with_script(settings, SCRIPT)
    }

    fn with_script(settings: Settings, script: &'static str) -> Self {
        Self {
            settings: Mutex::new(settings),
            worker: Mutex::new(None),
            script,
        }
    }

    /// Changes the settings; a running worker is replaced on next use.
    pub fn configure(&self, settings: &Settings) {
        let mut current = self.settings.lock().unwrap_or_else(PoisonError::into_inner);
        if *current != *settings {
            current.clone_from(settings);
        }
    }

    /// Stops the worker, freeing the model's memory.
    pub fn shut_down(&self) {
        *self.worker.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// Makes the background of `image` transparent.
    pub fn remove(&self, image: &DynamicImage) -> Result<DynamicImage, Error> {
        let settings = self
            .settings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let mut guard = self.worker.lock().unwrap_or_else(PoisonError::into_inner);
        if guard
            .as_ref()
            .is_some_and(|worker| worker.settings != settings)
        {
            *guard = None;
        }
        if guard.is_none() {
            *guard = Some(Worker::start(&settings, self.script)?);
        }
        let Some(worker) = guard.as_mut() else {
            return Err(Error::Invalid(
                "the background remover did not start".into(),
            ));
        };
        let (width, height) = (worker.width, worker.height);
        let input = tensor(image, width, height);
        match worker.run(&input) {
            Ok(logits) => Ok(apply_mask(image, &logits, width, height)),
            Err(error) => {
                // A failed worker is started afresh for the next picture.
                *guard = None;
                Err(error)
            }
        }
    }
}

/// The model input: the picture on white, resized, normalized, planar RGB.
fn tensor(image: &DynamicImage, width: u32, height: u32) -> Vec<f32> {
    let flat = Operation::Flatten([255, 255, 255])
        .apply(image.clone())
        .unwrap_or_else(|_| image.clone());
    let rgb = flat
        .resize_exact(width, height, FilterType::Triangle)
        .to_rgb8();
    let plane = width as usize * height as usize;
    let mut out = vec![0.0; 3 * plane];
    for (index, pixel) in rgb.pixels().enumerate() {
        for channel in 0..3 {
            out[channel * plane + index] =
                (f32::from(pixel.0[channel]) / 255.0 - MEAN[channel]) / STD[channel];
        }
    }
    out
}

/// Multiplies the mask the logits describe into the picture's alpha, at the
/// picture's own size and bit depth.
fn apply_mask(image: &DynamicImage, logits: &[f32], width: u32, height: u32) -> DynamicImage {
    let mask = GrayImage::from_fn(width, height, |x, y| {
        let logit = logits
            .get(y as usize * width as usize + x as usize)
            .copied()
            .unwrap_or(0.0);
        let opacity = 1.0 / (1.0 + (-logit.clamp(-40.0, 40.0)).exp());
        Luma([(opacity * 255.0).round() as u8])
    });
    let mask = image::imageops::resize(&mask, image.width(), image.height(), FilterType::Triangle);
    let weight = |x: u32, y: u32| u32::from(mask.get_pixel(x, y).0[0]);
    match image.color() {
        ColorType::L16 | ColorType::La16 | ColorType::Rgb16 | ColorType::Rgba16 => {
            let mut out = image.to_rgba16();
            for (x, y, pixel) in out.enumerate_pixels_mut() {
                let alpha = (u32::from(pixel.0[3]) * weight(x, y) + 127) / 255;
                pixel.0[3] = alpha as u16;
            }
            DynamicImage::ImageRgba16(out)
        }
        ColorType::Rgb32F | ColorType::Rgba32F => {
            let mut out = image.to_rgba32f();
            for (x, y, pixel) in out.enumerate_pixels_mut() {
                pixel.0[3] *= weight(x, y) as f32 / 255.0;
            }
            DynamicImage::ImageRgba32F(out)
        }
        _ => {
            let mut out = image.to_rgba8();
            for (x, y, pixel) in out.enumerate_pixels_mut() {
                let alpha = (u32::from(pixel.0[3]) * weight(x, y) + 127) / 255;
                pixel.0[3] = alpha as u8;
            }
            DynamicImage::ImageRgba8(out)
        }
    }
}

/// The state of a model download.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DownloadState {
    /// Bytes received.
    pub received: u64,
    /// Bytes expected.
    pub total: u64,
    /// The download has ended, well or not.
    pub finished: bool,
    /// Why it failed.
    pub error: Option<String>,
}

/// A model download in the background, through the worker script.
pub struct Download {
    state: Arc<Mutex<DownloadState>>,
    child: Arc<Mutex<Option<Child>>>,
    target: PathBuf,
}

impl std::fmt::Debug for Download {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Download")
            .field("target", &self.target)
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

impl Download {
    /// Starts downloading the variant of `settings` to its default file.
    /// `notify` is called on progress (to repaint the host).
    pub fn start(settings: &Settings, notify: impl Fn() + Send + 'static) -> Result<Self, Error> {
        let target = default_model(settings.variant);
        if target.exists() {
            return Err(Error::Invalid(format!(
                "{} already exists",
                target.display()
            )));
        }
        let scratch = Scratch::new(SCRIPT)?;
        let (child, lines) = spawn(
            &settings.python_path(),
            &scratch,
            &[
                OsStr::new("download"),
                OsStr::new("--model"),
                target.as_os_str(),
                OsStr::new("--variant"),
                OsStr::new(settings.variant.id()),
            ],
        )?;
        let state = Arc::new(Mutex::new(DownloadState::default()));
        let child = Arc::new(Mutex::new(Some(child)));
        let (shared, process) = (Arc::clone(&state), Arc::clone(&child));
        let _ = std::thread::Builder::new()
            .name("image-gallery-birefnet-download".into())
            .spawn(move || {
                let mut done = false;
                let mut error = None;
                for line in lines {
                    let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                        continue;
                    };
                    let mut state = shared.lock().unwrap_or_else(PoisonError::into_inner);
                    if let Some(received) =
                        value.get("received").and_then(serde_json::Value::as_u64)
                    {
                        state.received = received;
                        state.total = value
                            .get("total")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0);
                    }
                    if let Some(message) = value.get("error").and_then(serde_json::Value::as_str) {
                        error = Some(message.to_owned());
                    }
                    done |= value.get("done").is_some();
                    drop(state);
                    notify();
                }
                if let Some(mut child) = process
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take()
                {
                    let _ = child.wait();
                }
                let mut state = shared.lock().unwrap_or_else(PoisonError::into_inner);
                state.finished = true;
                state.error = error.or_else(|| {
                    (!done).then(|| {
                        let tail = scratch.log_tail();
                        if tail.is_empty() {
                            "the download stopped".to_owned()
                        } else {
                            tail
                        }
                    })
                });
                drop(state);
                notify();
            });
        Ok(Self {
            state,
            child,
            target,
        })
    }

    /// The progress so far.
    pub fn state(&self) -> DownloadState {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The file being downloaded.
    pub fn target(&self) -> &Path {
        &self.target
    }

    /// Stops the download; the partial file is not published.
    pub fn cancel(&self) {
        if let Some(child) = self
            .child
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
        {
            let _ = child.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    /// A stand-in for the worker, in plain Python: the logits say "keep"
    /// where the input's red channel is above average.
    const FAKE: &str = include_str!("../tests/fake_birefnet.py");

    fn python_available() -> bool {
        Command::new(python())
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn half_red() -> DynamicImage {
        let mut image = RgbaImage::from_pixel(200, 100, Rgba([0, 0, 255, 255]));
        for x in 0..100 {
            for y in 0..100 {
                image.put_pixel(x, y, Rgba([255, 0, 0, 255]));
            }
        }
        DynamicImage::ImageRgba8(image)
    }

    #[test]
    fn tensors_are_planar_and_normalized() {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(4, 2, Rgba([255, 0, 0, 0])));
        let tensor = tensor(&image, 2, 2);
        assert_eq!(tensor.len(), 12);
        // Transparent pixels are composed onto white first.
        let white = (1.0 - MEAN[1]) / STD[1];
        assert!((tensor[4] - white).abs() < 1e-5, "{}", tensor[4]);
    }

    #[test]
    fn masks_scale_into_alpha_at_the_source_depth() {
        let image = DynamicImage::ImageRgb16(half_red().to_rgb16());
        // A 2x1 mask: keep the left, drop the right.
        let masked = apply_mask(&image, &[40.0, -40.0], 2, 1);
        assert_eq!(masked.color(), ColorType::Rgba16);
        let rgba = masked.to_rgba16();
        assert_eq!(rgba.get_pixel(10, 50).0[3], u16::MAX);
        assert_eq!(rgba.get_pixel(190, 50).0[3], 0);
        assert_eq!(masked.width(), 200);
    }

    #[test]
    fn a_missing_model_is_reported_without_starting_python() {
        let settings = Settings {
            model: Some(PathBuf::from("/nonexistent/birefnet.onnx")),
            ..Settings::default()
        };
        let error = Remover::new(settings)
            .remove(&half_red())
            .expect_err("no model");
        assert!(error.to_string().contains("not found"), "{error}");
    }

    #[test]
    fn the_worker_protocol_round_trips() {
        if !python_available() {
            eprintln!("skipped: no Python interpreter");
            return;
        }
        let folder =
            std::env::temp_dir().join(format!("image-gallery-fake-model-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("folder");
        let model = folder.join("fake.onnx");
        std::fs::write(&model, b"not a real model").expect("model");
        let remover = Remover::with_script(
            Settings {
                model: Some(model.clone()),
                ..Settings::default()
            },
            FAKE,
        );
        for _ in 0..2 {
            let cut = remover.remove(&half_red()).expect("removes");
            let rgba = cut.to_rgba8();
            assert_eq!(rgba.get_pixel(20, 50).0[3], 255);
            assert_eq!(rgba.get_pixel(180, 50).0[3], 0);
        }
        // A change of settings restarts the worker; a broken model reports
        // the worker's own message.
        std::fs::write(&model, b"broken").expect("model");
        remover.configure(&Settings {
            model: Some(model),
            gpu: true,
            ..Settings::default()
        });
        let error = remover.remove(&half_red()).expect_err("fails");
        assert!(error.to_string().contains("broken model"), "{error}");
        remover.shut_down();
        let _ = std::fs::remove_dir_all(&folder);
    }
}
