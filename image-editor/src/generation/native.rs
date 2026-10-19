//! Direct use of Plakat's supported builder API, with inference off the UI thread.
use super::{Candidate, Mode, Request};
use crate::{Document, EditError, Raster};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

static BUSY: AtomicBool = AtomicBool::new(false);
struct Permit;
impl Drop for Permit {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}

/// A direct in-process Plakat job. Cancellation discards results at the next API boundary.
/// Plakat's supported API cannot interrupt an active denoising pass.
pub struct NativeJob {
    receiver: mpsc::Receiver<Result<Candidate, EditError>>,
    cancelled: Arc<AtomicBool>,
    finished: bool,
}
impl NativeJob {
    /// Whether an in-process inference call is still running, including discarded jobs.
    pub fn is_busy() -> bool {
        BUSY.load(Ordering::Acquire)
    }
    /// Start a library-backed inference job. No plakat executable is required.
    /// Set `PLAKAT_CACHE_DIR` before starting host threads; runtime cache switching is rejected.
    pub fn start(request: Request, doc: &Document) -> Result<Self, EditError> {
        Self::start_with_runtime(request, doc, &mufoed_model_runtime::Runtime::global())
    }
    /// Submit to the host's shared text/vision/image runtime.
    pub fn start_with_runtime(
        mut request: Request,
        doc: &Document,
        manager: &mufoed_model_runtime::Runtime,
    ) -> Result<Self, EditError> {
        request.validate(doc)?;
        if let Some(cache) = request.cache_dir.as_mut() {
            *cache = std::path::absolute(&*cache)?;
            let configured = std::env::var_os("PLAKAT_CACHE_DIR")
                .map(std::path::absolute)
                .transpose()?;
            if configured.as_ref() != Some(cache) {
                return Err(EditError::Invalid("Library backend uses startup PLAKAT_CACHE_DIR; restart the host with the selected cache".into()));
            }
        }
        #[cfg(windows)]
        if std::env::var_os("PLAKAT_NO_SDPA").is_none() {
            return Err(EditError::Invalid(
                "Set PLAKAT_NO_SDPA=1 before launching this Plakat revision on Windows/CUDA".into(),
            ));
        }
        if BUSY
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(EditError::Invalid(
                "Plakat is still finishing the previous library job".into(),
            ));
        }
        let permit = Permit;
        let ticket = manager
            .enqueue(mufoed_model_runtime::Task::new(
                "image",
                format!("plakat:{}", request.model),
            ))
            .map_err(|e| EditError::Invalid(e.to_string()))?;
        let source = doc.flatten();
        let mask = doc.mask().to_vec();
        let mask_raster = doc.mask_raster();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("image-studio-plakat".into())
            .spawn(move || {
                let _permit = permit;
                let result = ticket
                    .wait(&worker_cancelled, std::time::Duration::from_secs(900))
                    .map_err(|e| EditError::Invalid(e.to_string()))
                    .and_then(|_lease| {
                        run(request, source, &mask, &mask_raster, &worker_cancelled)
                    });
                let _ = sender.send(result);
            })?;
        Ok(Self {
            receiver,
            cancelled,
            finished: false,
        })
    }
    /// Poll for a candidate without blocking the UI thread.
    pub fn poll(&mut self) -> Result<Option<Candidate>, EditError> {
        if self.finished {
            return Ok(None);
        }
        match self.receiver.try_recv() {
            Ok(result) => {
                self.finished = true;
                if self.cancel_requested() {
                    return Err(EditError::Invalid(
                        "Library generation cancelled; result discarded".into(),
                    ));
                }
                result.map(Some)
            }
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.finished = true;
                Err(EditError::Invalid(
                    "Plakat library worker stopped unexpectedly".into(),
                ))
            }
        }
    }
    /// Request result discard. Inference keeps running until Plakat returns from its current call.
    pub fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
    /// Whether the worker's result has been marked for discard.
    pub fn cancel_requested(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}
impl Drop for NativeJob {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn run(
    request: Request,
    source: Raster,
    mask: &[u8],
    mask_raster: &Raster,
    cancelled: &AtomicBool,
) -> Result<Candidate, EditError> {
    let error = |e: String| EditError::Invalid(format!("Plakat library: {e}"));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let input = tempfile::tempdir()?;
    source.save_png(input.path().join("input.png"))?;
    mask_raster.save_png(input.path().join("mask.png"))?;
    if cancelled.load(Ordering::Acquire) {
        return Err(error("Cancelled before inference".into()));
    }
    if request.vae.is_some() {
        let raster = runtime.block_on(super::vae_compat::run(&request, &source, input.path()))?;
        return super::finish_candidate(raster, request, source, mask);
    }
    let images = runtime
        .block_on(async {
            if request.mode == Mode::TextToImage {
                let mut builder = plakat::api::Generate::new(&request.model)
                    .prompt(&request.prompt)
                    .negative(&request.negative)
                    .size(
                        request
                            .output_size
                            .unwrap_or([source.width(), source.height()])[0],
                        request
                            .output_size
                            .unwrap_or([source.width(), source.height()])[1],
                    )
                    .steps(request.steps as usize)
                    .guidance(f64::from(request.guidance))
                    .seed(request.seed);
                for lora in request.loras.iter().filter(|l| l.enabled) {
                    builder = builder.lora(lora.path.to_string_lossy(), lora.weight);
                }
                builder.run().await
            } else {
                let mut builder =
                    plakat::api::Img2img::new(&request.model, input.path().join("input.png"))
                        .prompt(&request.prompt)
                        .negative(&request.negative)
                        .strength(request.strength)
                        .steps(request.steps as usize)
                        .guidance(f64::from(request.guidance))
                        .seed(request.seed);
                if request.mode == Mode::Inpaint {
                    builder = builder
                        .mask(input.path().join("mask.png"))
                        .mask_feather(request.mask_feather)
                        .mask_invert(request.mask_invert);
                }
                for lora in request.loras.iter().filter(|l| l.enabled) {
                    builder = builder.lora(lora.path.to_string_lossy(), lora.weight);
                }
                builder.run().await
            }
        })
        .map_err(|e| error(format!("{e:#}")))?;
    if images.len() != 1 {
        return Err(error(format!("Expected one image, got {}", images.len())));
    }
    let image = &images[0];
    let rgba = image
        .pixels()
        .chunks_exact(3)
        .flat_map(|p| [p[0], p[1], p[2], 255])
        .collect();
    super::finish_candidate(
        Raster::new(image.width(), image.height(), rgba)?,
        request,
        source,
        mask,
    )
}
