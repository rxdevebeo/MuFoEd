//! Transactional generation with same-size postprocessing or pixel-space Hires Fix.
use super::{Candidate, Job, Mode, Request};
use crate::{model_tools::ModelJob, Document, EditError, Raster};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};

/// Pixel-space Hires Fix: `UltraSharp` enlargement followed by img2img at the target size.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HiresFix {
    /// Final dimensions, divisible by eight, larger than the first pass.
    pub target_size: [u32; 2],
    /// Guidance for the second diffusion pass.
    pub guidance: f32,
}
impl Default for HiresFix {
    fn default() -> Self {
        Self {
            target_size: [1536, 1536],
            guidance: 7.0,
        }
    }
}
/// Reproducible postprocessing, stored only on a completed pipeline candidate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Postprocess {
    /// None retains historical img2img then same-size sharpening.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hires: Option<HiresFix>,
    /// Img2img denoise: 0..0.5 for legacy postprocessing, 0..1 for Hires Fix.
    pub denoise: f32,
    /// Refiner scheduler steps (actual denoising steps depend on denoise).
    pub steps: u32,
    /// Local 4x-UltraSharp weights, not embedded in a project.
    pub ultrasharp: PathBuf,
    /// Source tile width; neighboring context is retained while downsampling.
    pub tile: u32,
    /// Use CUDA explicitly; false selects CPU explicitly.
    pub auto_device: bool,
}
impl Default for Postprocess {
    fn default() -> Self {
        let root = std::env::var_os("IMAGE_STUDIO_CACHE")
            .map(PathBuf::from)
            .and_then(|p| p.parent().map(Path::to_owned))
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".runtime"));
        let prepared = root.join("Upscale/ultrasharp/4x-UltraSharp.safetensors");
        let legacy = PathBuf::from("D:/Models/4xUltrasharp_4xUltrasharpV10.pt");
        Self {
            hires: None,
            denoise: 0.2,
            steps: 20,
            ultrasharp: if prepared.is_file() || !legacy.is_file() {
                prepared
            } else {
                legacy
            },
            tile: 256,
            auto_device: true,
        }
    }
}
impl Postprocess {
    /// Validate serializable parameters without checking local model files.
    pub fn validate_parameters(&self) -> Result<(), EditError> {
        if !self.denoise.is_finite()
            || !(0.0..=if self.hires.is_some() { 1.0 } else { 0.5 }).contains(&self.denoise)
            || !(1..=150).contains(&self.steps)
            || ![128, 256, 512].contains(&self.tile)
        {
            return Err(EditError::Invalid(
                "Invalid postprocessing denoise, steps or tile".into(),
            ));
        }
        if let Some(hires) = &self.hires {
            let [w, h] = hires.target_size;
            if w == 0
                || h == 0
                || w > 16384
                || h > 16384
                || !w.is_multiple_of(8)
                || !h.is_multiple_of(8)
                || u64::from(w) * u64::from(h) > 16_777_216
                || !hires.guidance.is_finite()
                || !(0.0..=30.0).contains(&hires.guidance)
            {
                return Err(EditError::Invalid(
                    "Invalid Hires Fix target size or guidance".into(),
                ));
            }
        }
        Ok(())
    }
    fn validate(&self) -> Result<(), EditError> {
        self.validate_parameters()?;
        if !self.ultrasharp.is_file() {
            return Err(EditError::Invalid("Выберите существующие веса 4x-UltraSharp для постобработки (вкладка Апскейл → загрузка модели)".into()));
        }
        Ok(())
    }
    fn args(&self) -> Vec<OsString> {
        let mut args = vec![
            "upscale".into(),
            "--variant".into(),
            "ultrasharp".into(),
            "--model".into(),
            self.ultrasharp.clone().into_os_string(),
            "--tile".into(),
            self.tile.to_string().into(),
            "--device".into(),
            if self.auto_device { "auto" } else { "cpu" }.into(),
        ];
        if let Some(hires) = &self.hires {
            args.extend([
                "--target-width".into(),
                hires.target_size[0].to_string().into(),
                "--target-height".into(),
                hires.target_size[1].to_string().into(),
            ]);
        } else {
            args.push("--keep-size".into());
        }
        args
    }
}
type Status = Arc<Mutex<&'static str>>;
fn cancelled(cancel: &AtomicBool) -> Result<(), EditError> {
    if cancel.load(Ordering::Acquire) {
        Err(EditError::Invalid("Generation pipeline cancelled".into()))
    } else {
        Ok(())
    }
}
fn stage(status: &Status, value: &'static str) {
    *status
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = value;
}
/// A complete asynchronous chain. Only the final result can be accepted by the host.
/// Each backend acquires its own shared-runtime lease; no lease is held across nested jobs.
pub struct PipelineJob {
    receiver: mpsc::Receiver<Result<Candidate, EditError>>,
    cancel: Arc<AtomicBool>,
    status: Status,
    finished: bool,
}
enum Inference {
    Cli(Box<Job>),
    #[cfg(feature = "plakat")]
    Native(super::NativeJob),
}
impl Inference {
    fn run(
        library: bool,
        executable: &Path,
        request: Request,
        doc: &Document,
        runtime: &mufoed_model_runtime::Runtime,
        cancel: &AtomicBool,
    ) -> Result<Candidate, EditError> {
        cancelled(cancel)?;
        let mut job = {
            #[cfg(feature = "plakat")]
            if library {
                Self::Native(super::NativeJob::start_with_runtime(request, doc, runtime)?)
            } else {
                Self::Cli(Box::new(Job::start_with_runtime(
                    executable, request, doc, runtime,
                )?))
            }
            #[cfg(not(feature = "plakat"))]
            {
                let _ = library;
                Self::Cli(Box::new(Job::start_with_runtime(
                    executable, request, doc, runtime,
                )?))
            }
        };
        loop {
            cancelled(cancel)?; // Drop forwards cancellation while backend retains cleanup ownership.
            let result = match &mut job {
                Self::Cli(j) => j.poll(),
                #[cfg(feature = "plakat")]
                Self::Native(j) => j.poll(),
            };
            if let Some(candidate) = result? {
                return Ok(candidate);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
fn process(
    mut request: Request,
    doc: &Document,
    cancel: &AtomicBool,
    status: &Status,
    mut generate: impl FnMut(Request, &Document) -> Result<Candidate, EditError>,
    mut sharpen: impl FnMut(&Raster, &Postprocess) -> Result<Raster, EditError>,
) -> Result<Candidate, EditError> {
    let _timing = crate::profiling::scope("pipeline.total");
    let post = request.postprocess.take();
    cancelled(cancel)?;
    stage(
        status,
        if post.is_some() {
            "1/3 · Генерация"
        } else {
            "Генерация"
        },
    );
    let first_timing = crate::profiling::scope("pipeline.first");
    let mut primary = generate(request, doc)?;
    drop(first_timing);
    cancelled(cancel)?;
    if let Some(post) = post {
        let image = if let Some(hires) = &post.hires {
            stage(status, "2/3 · Hires Fix · UltraSharp с увеличением");
            let upscaled = sharpen(&primary.raster, &post)?;
            cancelled(cancel)?;
            check_size(&upscaled, hires.target_size)?;
            stage(status, "3/3 · Hires Fix · Img2Img в итоговом размере");
            let refined = refine(&primary.request, upscaled, &post, &mut generate)?;
            check_size(&refined.raster, hires.target_size)?;
            refined.raster
        } else {
            stage(status, "2/3 · Img2Img · низкий denoise");
            let refined = refine(&primary.request, primary.raster, &post, &mut generate)?;
            cancelled(cancel)?;
            stage(status, "3/3 · UltraSharp · исходный размер");
            let image = sharpen(&refined.raster, &post)?;
            check_size(&image, [refined.raster.width(), refined.raster.height()])?;
            image
        };
        cancelled(cancel)?;
        primary.request.postprocess = Some(post);
        // Legacy inpaint protection is restored only after every postprocessing stage.
        primary = super::finish_candidate(image, primary.request, primary.source, doc.mask())?;
    }
    cancelled(cancel)?;
    stage(status, "Готово к просмотру");
    Ok(primary)
}
fn check_size(image: &Raster, expected: [u32; 2]) -> Result<(), EditError> {
    if [image.width(), image.height()] != expected {
        return Err(EditError::Invalid(
            "Pipeline stage returned unexpected output dimensions".into(),
        ));
    }
    Ok(())
}
fn refine(
    request: &Request,
    image: Raster,
    post: &Postprocess,
    generate: &mut impl FnMut(Request, &Document) -> Result<Candidate, EditError>,
) -> Result<Candidate, EditError> {
    let _timing = crate::profiling::scope("pipeline.refine");
    let doc = Document::from_raster(image)?;
    let mut refine = request.clone();
    refine.mode = Mode::ImageToImage;
    refine.output_size = None;
    refine.strength = post.denoise;
    refine.steps = post.steps;
    refine.mask_feather = 0;
    refine.mask_invert = false;
    refine.postprocess = None;
    if let Some(hires) = &post.hires {
        refine.guidance = hires.guidance;
    }
    generate(refine, &doc)
}
impl PipelineJob {
    /// Start using an embedded backend or CLI, and the host's common model runtime.
    pub fn start_with_runtime(
        library: bool,
        executable: impl AsRef<Path>,
        request: Request,
        doc: &Document,
        runtime: &mufoed_model_runtime::Runtime,
    ) -> Result<Self, EditError> {
        request.validate(doc)?;
        if let Some(post) = &request.postprocess {
            post.validate()?;
        }
        #[cfg(not(feature = "plakat"))]
        if library {
            return Err(EditError::Invalid(
                "Embedded Plakat feature is unavailable".into(),
            ));
        }
        let doc = doc.clone();
        let runtime = runtime.clone();
        let executable = executable.as_ref().to_owned();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let status = Arc::new(Mutex::new("Ожидание генерации"));
        let worker_status = status.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("image-generation-pipeline".into())
            .spawn(move || {
                let result = process(
                    request,
                    &doc,
                    &worker_cancel,
                    &worker_status,
                    |request, doc| {
                        Inference::run(library, &executable, request, doc, &runtime, &worker_cancel)
                    },
                    |image, post| {
                        let _timing = crate::profiling::scope("pipeline.ultrasharp");
                        let mut job = ModelJob::start_with_runtime(
                            "upscale_tool.py",
                            &post.args(),
                            Some(image),
                            None,
                            &runtime,
                        )?;
                        loop {
                            cancelled(&worker_cancel)?;
                            if let Some(output) = job.poll()? {
                                return output.raster.ok_or_else(|| {
                                    EditError::Invalid("UltraSharp returned no image".into())
                                });
                            }
                            std::thread::sleep(Duration::from_millis(20));
                        }
                    },
                );
                let _ = sender.send(result);
            })?;
        Ok(Self {
            receiver,
            cancel,
            status,
            finished: false,
        })
    }
    /// Nonblocking completion; intermediate images are never published as candidates.
    pub fn poll(&mut self) -> Result<Option<Candidate>, EditError> {
        if self.finished {
            return Ok(None);
        }
        if let Err(error) = cancelled(&self.cancel) {
            self.finished = true;
            return Err(error);
        }
        match self.receiver.try_recv() {
            Ok(result) => {
                self.finished = true;
                result.map(Some)
            }
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.finished = true;
                Err(EditError::Invalid(
                    "Generation pipeline worker stopped".into(),
                ))
            }
        }
    }
    /// Cancel the chain; active backend workers retain admission until cleanup completes.
    pub fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
    /// Current chain stage, separate from the backend's measured model lifecycle.
    pub fn stage(&self) -> &'static str {
        *self
            .status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
impl Drop for PipelineJob {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    fn candidate(request: Request, doc: &Document, color: [u8; 4]) -> Result<Candidate, EditError> {
        let [w, h] = request.size(doc);
        Ok(Candidate {
            request,
            source: doc.flatten(),
            raster: Raster::solid(w, h, color)?,
        })
    }
    fn status() -> Status {
        Arc::new(Mutex::new("test"))
    }
    #[test]
    fn stages_use_generated_pixels_preserve_seed_size_recipe_and_original_source() {
        let doc = Document::new(8, 8).unwrap();
        let before = doc.clone();
        let seen = RefCell::new(Vec::new());
        let request = Request {
            prompt: "vase".into(),
            seed: u64::MAX,
            output_size: Some([16, 8]),
            postprocess: Some(Postprocess::default()),
            ..Request::default()
        };
        let result = process(
            request.clone(),
            &doc,
            &AtomicBool::new(false),
            &status(),
            |r, d| {
                if !seen.borrow().is_empty() {
                    assert_eq!(d.flatten().pixels()[0], 70);
                    assert_eq!(r.mode, Mode::ImageToImage);
                    assert_eq!(r.strength.to_bits(), 0.2_f32.to_bits());
                    assert_eq!(r.steps, 20);
                    assert_eq!(r.seed, u64::MAX);
                }
                assert!(r.postprocess.is_none());
                seen.borrow_mut().push(r.clone());
                candidate(r, d, [70, 80, 90, 255])
            },
            |r, _| {
                assert_eq!((r.width(), r.height()), (16, 8));
                Raster::solid(16, 8, [10, 20, 30, 255])
            },
        )
        .unwrap();
        assert_eq!(seen.borrow().len(), 2);
        assert_eq!(result.request, request);
        assert_eq!(result.source, before.flatten());
        assert_eq!(doc, before);
    }
    #[test]
    fn disabled_chain_is_single_pass_and_old_recipes_remain_disabled() {
        let old = serde_json::to_value(Request::default()).unwrap();
        assert!(old.get("postprocess").is_none());
        assert!(serde_json::from_value::<Request>(old)
            .unwrap()
            .postprocess
            .is_none());
        let mut calls = 0;
        let doc = Document::new(8, 8).unwrap();
        process(
            Request::default(),
            &doc,
            &AtomicBool::new(false),
            &status(),
            |r, d| {
                calls += 1;
                candidate(r, d, [20; 4])
            },
            |_, _| panic!("disabled sharpening"),
        )
        .unwrap();
        assert_eq!(calls, 1);
    }
    #[test]
    fn cancellation_and_post_error_publish_no_candidate_or_document_change() {
        let doc = Document::new(8, 8).unwrap();
        let before = doc.clone();
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let request = Request {
            postprocess: Some(Postprocess::default()),
            ..Request::default()
        };
        let result = process(
            request.clone(),
            &doc,
            &cancel,
            &status(),
            |r, d| {
                calls += 1;
                cancel.store(true, Ordering::Release);
                candidate(r, d, [20; 4])
            },
            |_, _| panic!("cancelled sharpening"),
        );
        assert!(result.is_err());
        assert_eq!(calls, 1);
        for at in 2..=3 {
            let cancel = AtomicBool::new(false);
            let calls = std::cell::Cell::new(0);
            let result = process(
                request.clone(),
                &doc,
                &cancel,
                &status(),
                |r, d| {
                    calls.set(calls.get() + 1);
                    if at == 2 && calls.get() == 2 {
                        cancel.store(true, Ordering::Release);
                    }
                    candidate(r, d, [20; 4])
                },
                |image, _| {
                    if at == 3 {
                        cancel.store(true, Ordering::Release);
                    }
                    Ok(image.clone())
                },
            );
            assert!(result.is_err(), "late cancellation stage {at}");
        }
        let result = process(
            request,
            &doc,
            &AtomicBool::new(false),
            &status(),
            |r, d| candidate(r, d, [20; 4]),
            |_, _| Err(EditError::Invalid("model load failure".into())),
        );
        assert!(result
            .err()
            .unwrap()
            .to_string()
            .contains("model load failure"));
        assert_eq!(doc, before);
    }
    #[test]
    fn final_inpaint_protects_original_pixels_after_both_post_stages() {
        let mut doc = Document::from_raster(Raster::solid(8, 8, [1, 2, 3, 90]).unwrap()).unwrap();
        doc.stroke([3.0, 3.0], [4.0, 3.0], 1.0, [0; 4], crate::Tool::Mask)
            .unwrap();
        let r = Request {
            mode: Mode::Inpaint,
            postprocess: Some(Postprocess::default()),
            ..Request::default()
        };
        let result = process(
            r,
            &doc,
            &AtomicBool::new(false),
            &status(),
            |r, d| candidate(r, d, [100; 4]),
            |r, _| Raster::solid(r.width(), r.height(), [200; 4]),
        )
        .unwrap();
        for (pixel, mask) in result.raster.pixels().chunks_exact(4).zip(doc.mask()) {
            if *mask == 0 {
                assert_eq!(pixel, [1, 2, 3, 90]);
            }
        }
        let saved = Document::from_generated(result.raster, result.request).unwrap();
        assert_eq!(Document::from_psd(&saved.to_psd().unwrap()).unwrap(), saved);
    }
    #[test]
    fn hires_orders_upscale_before_refine_and_retains_both_passes_in_psd_recipe() {
        let doc = Document::new(8, 8).unwrap();
        let original = doc.clone();
        let events = RefCell::new(Vec::new());
        let request = Request {
            prompt: "vase".into(),
            seed: u64::MAX,
            output_size: Some([8, 8]),
            guidance: 7.5,
            sampler: super::super::Sampler::Dpmpp2mKarras,
            postprocess: Some(Postprocess {
                hires: Some(HiresFix {
                    target_size: [16, 16],
                    guidance: 4.5,
                }),
                denoise: 0.4,
                steps: 14,
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = process(
            request.clone(),
            &doc,
            &AtomicBool::new(false),
            &status(),
            |r, d| {
                assert!(r.postprocess.is_none());
                assert_eq!(r.seed, u64::MAX);
                if r.mode == Mode::TextToImage {
                    events.borrow_mut().push("generate");
                    candidate(r, d, [70; 4])
                } else {
                    events.borrow_mut().push("refine");
                    assert_eq!([d.width(), d.height()], [16, 16]);
                    assert_eq!(d.flatten().pixels()[0], 100);
                    assert_eq!(r.strength.to_bits(), 0.4_f32.to_bits());
                    assert_eq!(r.guidance.to_bits(), 4.5_f32.to_bits());
                    assert_eq!(r.steps, 14);
                    candidate(r, d, [200; 4])
                }
            },
            |image, post| {
                events.borrow_mut().push("upscale");
                assert_eq!(image.width(), 8);
                assert_eq!(image.pixels()[0], 70);
                let args = post.args();
                assert!(args.iter().any(|a| a == "--target-width"));
                assert!(!args.iter().any(|a| a == "--keep-size"));
                Raster::solid(16, 16, [100; 4])
            },
        )
        .unwrap();
        assert_eq!(*events.borrow(), ["generate", "upscale", "refine"]);
        assert_eq!(result.request, request);
        assert_eq!(result.source, original.flatten());
        assert_eq!(doc, original);
        assert_eq!(result.request.final_size(&doc), [16, 16]);
        assert_eq!(result.raster.pixels()[0], 200);
        let saved = Document::from_generated(result.raster, result.request).unwrap();
        assert_eq!(Document::from_psd(&saved.to_psd().unwrap()).unwrap(), saved);
    }
    #[test]
    fn hires_cancel_and_invalid_upscale_never_publish_partial_output() {
        let doc = Document::new(8, 8).unwrap();
        let original = doc.clone();
        let request = Request {
            postprocess: Some(Postprocess {
                hires: Some(HiresFix {
                    target_size: [16, 16],
                    guidance: 7.0,
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        for late in [false, true] {
            let cancel = AtomicBool::new(false);
            let calls = std::cell::Cell::new(0);
            let result = process(
                request.clone(),
                &doc,
                &cancel,
                &status(),
                |r, d| {
                    calls.set(calls.get() + 1);
                    candidate(r, d, [30; 4])
                },
                |_, _| {
                    if late {
                        cancel.store(true, Ordering::Release);
                        Raster::solid(16, 16, [100; 4])
                    } else {
                        Raster::solid(8, 8, [100; 4])
                    }
                },
            );
            assert!(result.is_err());
            assert_eq!(calls.get(), 1);
            assert_eq!(doc, original);
        }
    }
    #[test]
    fn hires_preflight_rejects_invalid_mode_size_cfg_and_old_post_stays_same_size() {
        let doc = Document::new(64, 64).unwrap();
        let mut request = Request {
            prompt: "vase".into(),
            output_size: Some([64, 64]),
            postprocess: Some(Postprocess {
                hires: Some(HiresFix {
                    target_size: [96, 96],
                    guidance: 7.0,
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(request.validate(&doc).is_ok());
        request.mode = Mode::ImageToImage;
        assert!(request.validate(&doc).is_err());
        request.mode = Mode::TextToImage;
        for size in [[64, 64], [65, 96], [264, 96], [8192, 8192]] {
            request
                .postprocess
                .as_mut()
                .unwrap()
                .hires
                .as_mut()
                .unwrap()
                .target_size = size;
            assert!(request.validate(&doc).is_err());
        }
        let hires = request
            .postprocess
            .as_mut()
            .unwrap()
            .hires
            .as_mut()
            .unwrap();
        hires.target_size = [96, 96];
        hires.guidance = f32::NAN;
        assert!(request.validate(&doc).is_err());
        let old = serde_json::to_value(Postprocess::default()).unwrap();
        assert!(old.get("hires").is_none());
        let restored: Postprocess = serde_json::from_value(old).unwrap();
        assert!(restored.hires.is_none());
        assert!(restored.args().iter().any(|a| a == "--keep-size"));
    }
    #[test]
    fn missing_ultrasharp_rejects_before_queue_or_primary_generation() {
        let runtime = mufoed_model_runtime::Runtime::new(mufoed_model_runtime::Policy::default());
        let request = Request {
            prompt: "vase".into(),
            postprocess: Some(Postprocess {
                ultrasharp: PathBuf::from("missing-post-model.safetensors"),
                ..Postprocess::default()
            }),
            ..Request::default()
        };
        assert!(PipelineJob::start_with_runtime(
            false,
            "missing-executable",
            request,
            &Document::new(8, 8).unwrap(),
            &runtime
        )
        .is_err());
        assert!(runtime.snapshot().active.is_none());
        assert!(runtime.snapshot().queued.is_empty());
    }
}
