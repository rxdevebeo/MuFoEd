//! Plakat CLI adapter. Arguments are passed directly, never through a shell.
//! A job owns an isolated temporary directory and kills its process on cancellation/drop.

use crate::{Document, EditError, Raster};
mod pipeline;
pub use pipeline::{HiresFix, PipelineJob, Postprocess};
#[cfg(feature = "plakat")]
mod dpmpp;
#[cfg(feature = "plakat")]
mod managed_sd;
#[cfg(feature = "plakat")]
mod native;
#[cfg(feature = "plakat")]
mod plakat_unet;
#[cfg(feature = "plakat")]
mod prompt_tokens;
#[cfg(feature = "plakat")]
mod vae_tiles;
#[cfg(feature = "plakat")]
mod worker;
#[cfg(feature = "plakat")]
pub use native::NativeJob;
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

/// Conservative staged SD peak, including CPU tensor cache and activation workspace.
/// Recalibrate for other architectures; an unknown family must not claim a measured bound.
pub fn memory_estimate(request: &Request, size: [u32; 2]) -> mufoed_model_runtime::Memory {
    let pixels = u64::from(size[0]) * u64::from(size[1]);
    let xl = match request.family {
        Some(crate::assets::Family::Sdxl) => true,
        Some(crate::assets::Family::Sd15) => false,
        _ => request.model.to_lowercase().contains("xl"),
    };
    let weights = if xl { 7_u64 << 30 } else { 3_u64 << 30 };
    let loras = request
        .loras
        .iter()
        .filter(|l| l.enabled)
        .filter_map(|l| std::fs::metadata(&l.path).ok())
        .map(|m| m.len())
        .sum::<u64>();
    mufoed_model_runtime::Memory {
        ram: Some(weights * 2 + loras * 3 + (2 << 30)),
        vram: Some(
            (if xl { 6_u64 << 30 } else { 2_u64 << 30 })
                + pixels.saturating_mul(3 * 1024)
                + (1 << 30),
        ),
    }
}

fn save_mask(mask: &Raster, path: &Path) -> Result<(), EditError> {
    // Plakat interprets RGBA masks by alpha, so opaque grayscale RGBA would
    // regenerate the entire image. Persist L8 to preserve the actual mask values.
    let pixels = mask.pixels().chunks_exact(4).map(|p| p[0]).collect();
    image::GrayImage::from_raw(mask.width(), mask.height(), pixels)
        .expect("validated mask dimensions")
        .save(path)?;
    Ok(())
}

/// Upstream revision used to verify the CLI contract.
pub const PLAKAT_REFERENCE: &str = "ce0d67a45bc9b5f742c3772aef85afe0d15890f6";

/// Supported generation routes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    /// Generate from text without source pixels.
    TextToImage,
    /// Transform the flattened source canvas.
    ImageToImage,
    /// Transform only the white parts of the painted mask.
    Inpaint,
}

/// Diffusion sampling algorithm recorded in settings and generation recipes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sampler {
    /// Historical Plakat scheduler; missing fields in old recipes retain this behavior.
    #[default]
    Ddim,
    /// Second-order DPM-Solver++ midpoint with Karras sigmas and an exact final x0 step.
    Dpmpp2mKarras,
}

/// Reproducible generation parameters. Models and weights belong to Plakat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// Diffusion solver; old serialized recipes default to DDIM.
    #[serde(default)]
    pub sampler: Sampler,
    /// Completed postprocessing recipe; use `PipelineJob` to execute it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postprocess: Option<Postprocess>,
    /// Architecture established by checkpoint inspection or explicit model selection.
    #[serde(default)]
    pub family: Option<crate::assets::Family>,
    /// Text-to-image output dimensions; source transforms retain canvas coordinates.
    #[serde(default)]
    pub output_size: Option<[u32; 2]>,
    /// Enabled local `LoRA` files, merged in listed order.
    #[serde(default)]
    pub loras: Vec<Lora>,
    /// Prepared diffusers VAE weights; None uses the model's default.
    #[serde(default)]
    pub vae: Option<PathBuf>,
    /// Upstream mask feathering radius. Protected pixels are restored after inference.
    #[serde(default)]
    pub mask_feather: u32,
    /// Regenerate outside the painted mask.
    #[serde(default)]
    pub mask_invert: bool,
    /// Optional Hugging Face cache root, including prepared local checkpoints.
    pub cache_dir: Option<PathBuf>,
    /// Generation route.
    pub mode: Mode,
    /// Plakat model alias or Hugging Face repository identifier.
    pub model: String,
    /// Desired image description.
    pub prompt: String,
    /// Undesired image content.
    pub negative: String,
    /// Deterministic seed.
    pub seed: u64,
    /// Number of denoising steps, 1..150.
    pub steps: u32,
    /// Classifier-free guidance, 0..30.
    pub guidance: f32,
    /// Image-to-image denoising strength, 0..1.
    pub strength: f32,
}
impl Default for Request {
    fn default() -> Self {
        Self {
            sampler: Sampler::Ddim,
            postprocess: None,
            family: None,
            output_size: None,
            loras: Vec::new(),
            vae: None,
            mask_feather: 0,
            mask_invert: false,
            cache_dir: None,
            mode: Mode::TextToImage,
            model: "sd15".into(),
            prompt: String::new(),
            negative: String::new(),
            seed: 42,
            steps: 20,
            guidance: 7.5,
            strength: 0.6,
        }
    }
}
impl Request {
    /// Actual inference dimensions, without resizing source pixels or their mask.
    pub fn size(&self, doc: &Document) -> [u32; 2] {
        if self.mode == Mode::TextToImage {
            self.output_size.unwrap_or([doc.width(), doc.height()])
        } else {
            [doc.width(), doc.height()]
        }
    }
    /// Final output dimensions, including an optional Hires Fix second pass.
    pub fn final_size(&self, doc: &Document) -> [u32; 2] {
        self.expected_size([doc.width(), doc.height()])
    }
    fn expected_size(&self, source: [u32; 2]) -> [u32; 2] {
        if self.mode == Mode::TextToImage {
            self.postprocess
                .as_ref()
                .and_then(|p| p.hires.as_ref())
                .map_or_else(|| self.output_size.unwrap_or(source), |h| h.target_size)
        } else {
            source
        }
    }
    /// Reject invalid parameters before starting any process or downloading weights.
    pub fn validate(&self, doc: &Document) -> Result<(), EditError> {
        let invalid = |s: &str| EditError::Invalid(s.into());
        if self.prompt.trim().is_empty()
            || self.prompt.len() > 16384
            || self.negative.len() > 16384
            || self.model.trim().is_empty()
            || self.model.starts_with('-')
        {
            return Err(invalid("Enter a prompt and valid model identifier"));
        }
        if !(1..=150).contains(&self.steps)
            || !self.guidance.is_finite()
            || !(0.0..=30.0).contains(&self.guidance)
            || !self.strength.is_finite()
            || !(0.0..=1.0).contains(&self.strength)
        {
            return Err(invalid("Invalid generation parameters"));
        }
        let [width, height] = self.size(doc);
        if width == 0
            || height == 0
            || u64::from(width) * u64::from(height) > 16_777_216
            || !width.is_multiple_of(8)
            || !height.is_multiple_of(8)
        {
            return Err(invalid(
                "Generation dimensions must be divisible by 8; resize the canvas first",
            ));
        }
        if let Some(post) = &self.postprocess {
            post.validate_parameters()?;
            if let Some(hires) = &post.hires {
                if self.output_size.is_none() {
                    return Err(invalid("Hires Fix requires explicit first-pass dimensions"));
                }
                if self.mode != Mode::TextToImage {
                    return Err(invalid("Hires Fix requires text-to-image generation"));
                }
                let [w, h] = hires.target_size;
                if w < width || h < height || [w, h] == [width, height] {
                    return Err(invalid(
                        "Hires Fix target must be larger than the first pass",
                    ));
                }
                if u64::from(w) > u64::from(width) * 4 || u64::from(h) > u64::from(height) * 4 {
                    return Err(invalid(
                        "Hires Fix enlargement must not exceed 4x per dimension",
                    ));
                }
            }
        }
        if self.mask_feather > 128 || self.loras.len() > 16 {
            return Err(invalid("Too many LoRAs or excessive mask feathering"));
        }
        for lora in self.loras.iter().filter(|l| l.enabled) {
            if !lora.weight.is_finite()
                || !(-2.0..=2.0).contains(&lora.weight)
                || !lora.path.is_file()
            {
                return Err(invalid(
                    "LoRA must be an existing file with weight in -2..2",
                ));
            }
            let asset = crate::assets::inspect(&lora.path)?;
            if asset.kind != crate::assets::Kind::Lora {
                return Err(invalid("Selected file is not a LoRA"));
            }
            if self.family.is_some_and(|f| {
                f != crate::assets::Family::Unknown
                    && asset.family != crate::assets::Family::Unknown
                    && f != asset.family
            }) {
                return Err(invalid("LoRA architecture differs from the selected model"));
            }
        }
        if self.vae.as_ref().is_some_and(|p| !p.is_file()) {
            return Err(invalid("Selected VAE file is missing"));
        }
        if self.mode == Mode::Inpaint
            && !doc
                .mask()
                .iter()
                .any(|v| if self.mask_invert { *v < 255 } else { *v > 0 })
        {
            return Err(invalid("Paint an inpaint mask first (white = regenerate)"));
        }
        Ok(())
    }
    /// Build the verified CLI argument vector. Paths and prompt remain separate arguments.
    pub fn arguments(&self, doc: &Document, directory: &Path) -> Result<Vec<OsString>, EditError> {
        self.validate(doc)?;
        if self.sampler != Sampler::Ddim {
            return Err(EditError::Invalid("DPM++ 2M Karras requires the embedded backend; the pinned CLI uses a different solver".into()));
        }
        if self.vae.is_some() {
            return Err(EditError::Invalid(
                "Optional VAE requires the embedded Plakat backend".into(),
            ));
        }
        let mut args: Vec<OsString> = Vec::new();
        if let Some(cache) = &self.cache_dir {
            args.extend([OsString::from("--cache-dir"), cache.as_os_str().to_owned()]);
        }
        if self.mode == Mode::TextToImage {
            args.extend([OsString::from("generate"), OsString::from(&self.prompt)]);
        } else {
            args.extend([
                OsString::from("img2img"),
                directory.join("input.png").into_os_string(),
                OsString::from("--prompt"),
                OsString::from(&self.prompt),
            ]);
            args.extend([
                OsString::from("--strength"),
                self.strength.to_string().into(),
            ]);
            if self.mode == Mode::Inpaint {
                args.extend([
                    OsString::from("--mask"),
                    directory.join("mask.png").into_os_string(),
                    OsString::from("--mask-feather"),
                    self.mask_feather.to_string().into(),
                ]);
                if self.mask_invert {
                    args.push("--mask-invert".into());
                }
            }
        }
        for (key, value) in [
            ("--model", self.model.clone()),
            ("--negative", self.negative.clone()),
            ("--seed", self.seed.to_string()),
            ("--steps", self.steps.to_string()),
            ("--guidance", self.guidance.to_string()),
            (
                "--size",
                format!("{}x{}", self.size(doc)[0], self.size(doc)[1]),
            ),
        ] {
            args.extend([OsString::from(key), OsString::from(value)]);
        }
        for lora in self.loras.iter().filter(|l| l.enabled) {
            args.extend([
                OsString::from("--lora"),
                OsString::from(format!("{}:{}", lora.path.display(), lora.weight)),
            ]);
        }
        args.extend([
            OsString::from("--out"),
            directory.join("out").into_os_string(),
        ]);
        Ok(args)
    }
}

/// One reproducible `LoRA` attachment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lora {
    /// Local safetensors adapter weights.
    pub path: PathBuf,
    /// Merge strength in -2..2.
    pub weight: f32,
    /// Include this attachment in inference.
    pub enabled: bool,
}

/// An inference result awaiting explicit acceptance as a new layer.
pub struct Candidate {
    /// Generated pixels.
    pub raster: Raster,
    /// Exact parameters used by this job.
    pub request: Request,
    /// Source snapshot, used to reject applying transformations to an edited canvas.
    pub source: Raster,
}

/// A nonblocking, cancellable inference process.
pub struct Job {
    child: Option<Child>,
    command: Option<Command>,
    ticket: Option<mufoed_model_runtime::Ticket>,
    lease: Option<mufoed_model_runtime::Lease>,
    directory: tempfile::TempDir,
    request: Request,
    source: Raster,
    mask: Vec<u8>,
    finished: bool,
}
impl Job {
    /// Queue Plakat in an isolated directory; `poll` spawns after admission.
    /// Installing Plakat/models is an explicit setup step.
    pub fn start(
        executable: impl AsRef<Path>,
        request: Request,
        doc: &Document,
    ) -> Result<Self, EditError> {
        Self::start_with_runtime(
            executable,
            request,
            doc,
            &mufoed_model_runtime::Runtime::global(),
        )
    }
    /// Queue CLI inference through the host's common model runtime.
    pub fn start_with_runtime(
        executable: impl AsRef<Path>,
        mut request: Request,
        doc: &Document,
        runtime: &mufoed_model_runtime::Runtime,
    ) -> Result<Self, EditError> {
        if let Some(cache) = request.cache_dir.as_mut() {
            *cache = std::path::absolute(&*cache)?;
        }
        request.validate(doc)?;
        if request.postprocess.is_some() {
            return Err(EditError::Invalid(
                "Use PipelineJob for generation with postprocessing".into(),
            ));
        }
        let directory = tempfile::tempdir()?;
        let args = request.arguments(doc, directory.path())?;
        let source = doc.flatten();
        source.save_png(directory.path().join("input.png"))?;
        save_mask(&doc.mask_raster(), &directory.path().join("mask.png"))?;
        std::fs::create_dir(directory.path().join("out"))?;
        let log = File::create(directory.path().join("process.log"))?;
        let executable = executable.as_ref();
        let resolved = if executable.components().count() > 1 {
            std::path::absolute(executable)?
        } else {
            executable.to_owned()
        };
        let mut command = Command::new(resolved);
        #[cfg(windows)]
        command.env("PLAKAT_NO_SDPA", "1"); // Upstream's fused SDPA has a Metal implementation only.
        command
            .args(args)
            .current_dir(directory.path())
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut task =
            mufoed_model_runtime::Task::new("image", format!("plakat-cli:{}", request.model));
        task.memory = memory_estimate(&request, request.size(doc));
        task.memory.vram = task.memory.vram.map(|peak| peak + (2 << 30)); // CLI keeps all SD components resident.
        let ticket = runtime
            .enqueue(task)
            .map_err(|e| EditError::Invalid(e.to_string()))?;
        Ok(Self {
            child: None,
            command: Some(command),
            ticket: Some(ticket),
            lease: None,
            directory,
            request,
            source,
            mask: doc.mask().to_vec(),
            finished: false,
        })
    }
    /// Poll without blocking the UI. Inpaint results are clipped back to the exact saved mask.
    #[allow(clippy::cast_possible_truncation)] // Mask interpolation is bounded to 0..255.
    pub fn poll(&mut self) -> Result<Option<Candidate>, EditError> {
        if self.finished {
            return Ok(None);
        }
        if let Some(ticket) = &mut self.ticket {
            let Some(lease) = ticket
                .try_start()
                .map_err(|e| EditError::Invalid(e.to_string()))?
            else {
                return Ok(None);
            };
            self.lease = Some(lease);
            self.ticket = None;
        }
        if let Some(mut command) = self.command.take() {
            match command.spawn() {
                Ok(child) => self.child = Some(child),
                Err(error) => {
                    self.finished = true;
                    self.lease = None;
                    return Err(EditError::Invalid(format!(
                        "Cannot start Plakat: {error}. Set the path to plakat.exe."
                    )));
                }
            }
        }
        let Some(status) = self.child.as_mut().expect("active process").try_wait()? else {
            return Ok(None);
        };
        self.finished = true;
        self.child = None;
        self.lease = None;
        if !status.success() {
            return Err(EditError::Invalid(format!(
                "Plakat failed ({status}): {}",
                self.log_excerpt()
            )));
        }
        let output = self.directory.path().join("out");
        let mut paths: Vec<PathBuf> = std::fs::read_dir(output)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png"))
                    && !p
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().contains("preview"))
            })
            .collect();
        paths.sort();
        if paths.len() != 1 {
            return Err(EditError::Invalid(format!(
                "Expected one Plakat output PNG, got {}",
                paths.len()
            )));
        }
        finish_candidate(
            Raster::open(&paths[0])?,
            self.request.clone(),
            self.source.clone(),
            &self.mask,
        )
        .map(Some)
    }
    /// Kill and reap the inference process. Temporary inputs and outputs are removed on drop.
    pub fn cancel(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.ticket = None;
        self.command = None;
        self.lease = None;
        self.finished = true;
    }
    /// Read a bounded log excerpt for diagnostics.
    pub fn log_excerpt(&self) -> String {
        let mut text = String::new();
        if let Ok(file) = File::open(self.directory.path().join("process.log")) {
            let _ = file.take(8192).read_to_string(&mut text);
        }
        text
    }
}

#[allow(clippy::cast_possible_truncation)] // Normalized mask interpolation stays in 0..255.
fn finish_candidate(
    mut raster: Raster,
    request: Request,
    source: Raster,
    mask: &[u8],
) -> Result<Candidate, EditError> {
    let expected = request.expected_size([source.width(), source.height()]);
    if [raster.width(), raster.height()] != expected {
        return Err(EditError::Invalid(
            "Plakat returned different dimensions; result was not applied".into(),
        ));
    }
    if request.mode == Mode::Inpaint {
        let pixels: Vec<u8> = raster
            .pixels()
            .chunks_exact(4)
            .zip(source.pixels().chunks_exact(4))
            .zip(mask.iter())
            .flat_map(|((generated, source), mask)| {
                let m = u32::from(if request.mask_invert {
                    255 - *mask
                } else {
                    *mask
                });
                let mut out = [0; 4];
                for i in 0..4 {
                    out[i] =
                        ((u32::from(generated[i]) * m + u32::from(source[i]) * (255 - m) + 127)
                            / 255) as u8;
                }
                out
            })
            .collect();
        raster = Raster::new(raster.width(), raster.height(), pixels)?;
    }
    Ok(Candidate {
        raster,
        request,
        source,
    })
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protected_inpaint_pixels_retain_original_rgba_including_alpha() {
        let source = Raster::solid(8, 8, [12, 34, 56, 78]).unwrap();
        let generated = Raster::solid(8, 8, [200, 150, 100, 255]).unwrap();
        let mut mask = vec![0; 64];
        mask[1] = 255;
        let request = Request {
            mode: Mode::Inpaint,
            ..Request::default()
        };
        let candidate = finish_candidate(generated, request, source, &mask).unwrap();
        assert_eq!(&candidate.raster.pixels()[..4], &[12, 34, 56, 78]);
        assert_eq!(&candidate.raster.pixels()[4..8], &[200, 150, 100, 255]);
        assert_eq!(&candidate.raster.pixels()[8..12], &[12, 34, 56, 78]);
    }
    #[test]
    fn backend_size_mismatch_is_rejected_without_resampling() {
        assert!(finish_candidate(
            Raster::solid(16, 8, [0; 4]).unwrap(),
            Request::default(),
            Raster::solid(8, 8, [0; 4]).unwrap(),
            &[0; 64]
        )
        .is_err());
    }
    #[test]
    fn text_size_is_independent_but_transforms_use_source_coordinates() {
        let doc = Document::new(64, 64).unwrap();
        let mut request = Request {
            prompt: "vase".into(),
            output_size: Some([128, 72]),
            ..Request::default()
        };
        assert_eq!(request.size(&doc), [128, 72]);
        request.validate(&doc).unwrap();
        assert!(finish_candidate(
            Raster::solid(128, 72, [20; 4]).unwrap(),
            request.clone(),
            doc.flatten(),
            doc.mask()
        )
        .is_ok());
        request.mode = Mode::ImageToImage;
        assert_eq!(request.size(&doc), [64, 64]);
        assert!(finish_candidate(
            Raster::solid(128, 72, [20; 4]).unwrap(),
            request,
            doc.flatten(),
            doc.mask()
        )
        .is_err());
    }
    #[test]
    fn inverted_mask_retains_painted_rgba() {
        let source = Raster::solid(8, 8, [1, 2, 3, 4]).unwrap();
        let generated = Raster::solid(8, 8, [90; 4]).unwrap();
        let mut mask = vec![0; 64];
        mask[0] = 255;
        let request = Request {
            mode: Mode::Inpaint,
            mask_invert: true,
            ..Request::default()
        };
        let result = finish_candidate(generated, request, source, &mask).unwrap();
        assert_eq!(&result.raster.pixels()[..4], &[1, 2, 3, 4]);
        assert_eq!(&result.raster.pixels()[4..8], &[90; 4]);
    }
    #[test]
    fn old_projects_keep_defaults_and_concrete_seed() {
        let old = r#"{"cache_dir":null,"mode":"TextToImage","model":"sd15","prompt":"vase","negative":"","seed":18446744073709551614,"steps":20,"guidance":7.5,"strength":0.6}"#;
        let request: Request = serde_json::from_str(old).unwrap();
        assert_eq!(request.seed, u64::MAX - 1);
        assert!(request.output_size.is_none());
        assert!(request.loras.is_empty());
        let doc = Document::from_generated(Raster::solid(8, 8, [1; 4]).unwrap(), request.clone())
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project.json");
        doc.save_project(&path).unwrap();
        assert_eq!(
            Document::open_project(path).unwrap().layers()[0].recipe(),
            Some(&request)
        );
    }
    #[test]
    fn backend_mask_is_luma_not_opaque_alpha() {
        let dir = tempfile::tempdir().unwrap();
        let mut doc = Document::new(16, 16).unwrap();
        doc.select_rectangle([2.0, 2.0], [6.0, 6.0]).unwrap();
        let path = dir.path().join("mask.png");
        save_mask(&doc.mask_raster(), &path).unwrap();
        let image = image::open(path).unwrap();
        assert_eq!(image.color(), image::ColorType::L8);
        assert_eq!(image.to_luma8().as_raw(), doc.mask());
    }
}
