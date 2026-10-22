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

/// A staged in-process Plakat job. Cancellation stops between components/denoise steps.
/// The current kernel finishes before admission or its buffers can be released.
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
        if request.postprocess.is_some() {
            return Err(EditError::Invalid(
                "Use PipelineJob for generation with postprocessing".into(),
            ));
        }
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
        let mut task =
            mufoed_model_runtime::Task::new("image", format!("plakat:{}", request.model));
        // Resolve immutable model identities before reserving the actual cold/warm peak.
        // Metadata itself needs no tensor allocations and uses a small CPU admission.
        task.device = "cpu".into();
        task.memory = mufoed_model_runtime::Memory {
            ram: Some(16 << 20),
            vram: Some(0),
        };
        let ticket = manager
            .enqueue(task)
            .map_err(|e| EditError::Invalid(e.to_string()))?;
        let source = doc.flatten();
        let mask = doc.mask().to_vec();
        let mask_raster = doc.mask_raster();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let (sender, receiver) = mpsc::channel();
        super::worker::dispatch(Box::new(move || {
            let worker_permit = permit;
            let result = ticket
                .wait(&worker_cancelled, std::time::Duration::from_secs(900))
                .map_err(|e| EditError::Invalid(e.to_string()))
                .and_then(|lease| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run(
                            request,
                            source,
                            &mask,
                            &mask_raster,
                            &worker_cancelled,
                            &lease,
                        )
                    }))
                    .unwrap_or_else(|_| {
                        lease.set_phase(mufoed_model_runtime::Phase::Failed);
                        let cleanup = lease.release_idle();
                        Err(EditError::Invalid(format!(
                            "Plakat worker panicked; cleanup: {cleanup:?}"
                        )))
                    })
                });
            drop(worker_permit); // The next pipeline stage may start as soon as the result is received.
            let _ = sender.send(result);
        }))?;
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

pub(super) fn estimate(request: &Request, size: [u32; 2]) -> mufoed_model_runtime::Memory {
    let memory = super::memory_estimate(request, size);
    #[cfg(feature = "plakat-cuda")]
    {
        let mut memory = memory;
        let pixels = u64::from(size[0]) * u64::from(size[1]);
        let xl = request.family == Some(crate::assets::Family::Sdxl)
            || (request.family.is_none() && request.model.to_lowercase().contains("xl"));
        let bounded = super::plakat_unet::bounded_attention(size, xl);
        if bounded || !xl {
            // Account both score + softmax F32 buffers using the execution policy.
            // Above the family threshold: one CFG branch, 2 heads, bounded query rows.
            // SD1.x at <=512² retains batched CFG and all 8 heads.
            let baseline = if xl { 6_u64 << 30 } else { 3_u64 << 30 };
            let tokens = pixels.div_ceil(if xl { 256 } else { 64 });
            let heads = if bounded {
                2
            } else if request.guidance > 1.0 {
                16
            } else {
                8
            };
            let query_rows = super::plakat_unet::attention_query_rows(tokens);
            memory.vram = Some(
                baseline
                    + pixels.saturating_mul(1024)
                    + query_rows.saturating_mul(tokens).saturating_mul(heads * 8),
            );
        }
        memory
    }
    #[cfg(not(feature = "plakat-cuda"))]
    {
        mufoed_model_runtime::Memory {
            ram: memory
                .ram
                .map(|v| v + (4 << 30) + u64::from(size[0]) * u64::from(size[1]) * 12 * 1024),
            vram: Some(0),
        }
    }
}

fn run(
    request: Request,
    source: Raster,
    mask: &[u8],
    mask_raster: &Raster,
    cancelled: &AtomicBool,
    lease: &mufoed_model_runtime::Lease,
) -> Result<Candidate, EditError> {
    if request.mode != Mode::TextToImage && request.strength == 0.0 {
        return super::finish_candidate(source.clone(), request, source, mask);
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let io_timing = crate::profiling::scope("native.input_png");
    let input = tempfile::tempdir()?;
    source.save_png(input.path().join("input.png"))?;
    super::save_mask(mask_raster, &input.path().join("mask.png"))?;
    drop(io_timing);
    let estimate = mufoed_model_runtime::Memory {
        ram: Some(16 << 20),
        vram: Some(0),
    };
    let raster = lease
        .retry(
            estimate,
            if estimate.vram == Some(0) {
                "cpu"
            } else {
                "auto"
            },
            cancelled,
            |_| {
                runtime
                    .block_on(super::managed_sd::run(
                        &request,
                        &source,
                        input.path(),
                        lease,
                        cancelled,
                    ))
                    .map_err(|e| format!("Plakat staged: {e:#}"))
            },
        )
        .map_err(EditError::Invalid)?;
    super::finish_candidate(raster, request, source, mask)
}

#[cfg(all(test, feature = "plakat-cuda"))]
mod tests {
    use super::*;
    #[test]
    fn sd15_hires_peak_uses_bounded_query_rows_not_a_full_score_matrix() {
        let request = Request {
            family: Some(crate::assets::Family::Sd15),
            ..Request::default()
        };
        assert_eq!(estimate(&request, [512, 512]).vram, Some(5_637_144_576));
        assert!(super::super::plakat_unet::bounded_attention(
            [768, 768],
            false
        ));
        assert!(super::super::plakat_unet::bounded_attention(
            [1024, 1024],
            false
        ));
        assert!(!super::super::plakat_unet::bounded_attention(
            [512, 512],
            false
        ));
        assert!(!super::super::plakat_unet::bounded_attention(
            [1024, 1024],
            true
        ));
        assert!(estimate(&request, [768, 768]).vram.unwrap() < 6_u64 << 30);
        assert_eq!(estimate(&request, [1024, 1024]).vram, Some(8_u64 << 30));
        let full_scores = (3_u64 << 30) + 1536 * 1536 * 1024 + 36_864 * 36_864 * 2 * 8;
        assert_eq!(full_scores, 27_380_416_512); // Exact user-reported failure.
        let bounded = (3_u64 << 30) + 1536 * 1536 * 1024 + 1024 * 36_864 * 2 * 8;
        assert_eq!(estimate(&request, [1536, 1536]).vram, Some(bounded));
        assert!(bounded < 11_548_701_492);
        assert_eq!(
            estimate(&request, [1536, 1536]),
            estimate(&request, [768, 3072])
        );
    }
    #[test]
    fn high_resolution_estimate_matches_sequential_cfg_and_bounded_vae() {
        let request = Request {
            family: Some(crate::assets::Family::Sdxl),
            ..Request::default()
        };
        assert_eq!(
            estimate(&request, [1024, 1024]),
            super::super::memory_estimate(&request, [1024, 1024])
        );
        let peak = estimate(&request, [1536, 1536]).vram.unwrap();
        assert_eq!(
            peak,
            (6_u64 << 30) + 1536 * 1536 * 1024 + 9216 * 9216 * 2 * 8
        );
        assert!(peak < 13_726_593_844); // Reported 5070 Ti budget, including host reserve.
        assert!(estimate(&request, [2048, 2048]).vram.unwrap() > 13_u64 << 30);
        assert_eq!(
            estimate(&request, [768, 3072]),
            estimate(&request, [1536, 1536])
        );
    }
}
