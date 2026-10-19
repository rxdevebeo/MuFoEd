//! Optional-VAE bridge for the pinned Plakat revision. Keep internal API usage isolated here.
use super::{Mode, Request};
use crate::{EditError, Raster};
use plakat::pipelines::{
    img2img,
    lora::LoraSpec,
    portrait,
    sd_core::{SdCore, SdLoadRequest, SdVariant},
    t2i,
};
use std::{path::Path, str::FromStr, sync::Arc};

#[allow(clippy::too_many_lines)] // Explicit pinned contracts for both inference routes.
pub(super) async fn run(
    request: &Request,
    source: &Raster,
    input: &Path,
) -> Result<Raster, EditError> {
    let error = |e: String| EditError::Invalid(format!("Plakat optional VAE: {e}"));
    let device = plakat::api::device("auto").map_err(|e| error(format!("{e:#}")))?;
    let dtype = if matches!(device, candle_core::Device::Cpu) {
        candle_core::DType::F32
    } else {
        candle_core::DType::F16
    };
    let [w, h] = if request.mode == Mode::TextToImage {
        request
            .output_size
            .unwrap_or([source.width(), source.height()])
    } else {
        [source.width(), source.height()]
    };
    let config = SdVariant::detect(&request.model).config(w as usize, h as usize);
    let vae = Arc::new(
        config
            .build_vae(request.vae.as_ref().expect("selected VAE"), &device, dtype)
            .map_err(|e| error(format!("{e:#}")))?,
    );
    let mut loras = Vec::new();
    for lora in request.loras.iter().filter(|l| l.enabled) {
        let mut spec = LoraSpec::from_str(&lora.path.to_string_lossy())
            .map_err(|e| error(format!("{e:#}")))?;
        spec.scale = lora.weight;
        loras.push(spec);
    }
    let out = input.join("vae-out");
    std::fs::create_dir(&out)?;
    if request.mode == Mode::TextToImage {
        let pipeline = t2i::Pipeline::load(t2i::LoadRequest {
            model: request.model.clone(),
            device,
            loras,
            lora_scale: 1.0,
            use_refiner: false,
            embeddings: Vec::new(),
            vae_cache: Some(vae),
        })
        .await
        .map_err(|e| error(format!("{e:#}")))?;
        let mut req = t2i::GenRequest::simple(
            request.prompt.clone(),
            request.negative.clone(),
            w,
            h,
            request.steps as usize,
            Some(request.seed),
            plakat::pipelines::scheduler::SchedulerKind::default(),
            out.clone(),
        );
        req.guidance = f64::from(request.guidance);
        pipeline
            .generate(&req, &[])
            .map_err(|e| error(format!("{e:#}")))?;
    } else {
        let mut resolved = Vec::new();
        for lora in &loras {
            resolved.push(lora.resolve().await.map_err(|e| error(format!("{e:#}")))?);
        }
        let core = SdCore::load(SdLoadRequest {
            model: request.model.clone(),
            device: device.clone(),
            loras: resolved,
            lora_scale: 1.0,
            embeddings: Vec::new(),
            vae_cache: Some(vae),
        })
        .await
        .map_err(|e| error(format!("{e:#}")))?;
        let pipeline = portrait::Pipeline::from_core(Arc::new(core));
        let req = img2img::Request {
            prompt: request.prompt.clone(),
            negative: request.negative.clone(),
            model: request.model.clone(),
            device,
            loras,
            lora_scale: 1.0,
            input: input.join("input.png"),
            mask: (request.mode == Mode::Inpaint).then(|| input.join("mask.png")),
            mask_feather: request.mask_feather,
            mask_invert: request.mask_invert,
            width: w,
            height: h,
            count: 1,
            steps: request.steps as usize,
            guidance: f64::from(request.guidance),
            scheduler: plakat::pipelines::scheduler::SchedulerKind::default(),
            strength: request.strength,
            seed: Some(request.seed),
            out_dir: out.clone(),
            controls: Vec::new(),
        };
        img2img::run_with_pipeline(&pipeline, &req)
            .await
            .map_err(|e| error(format!("{e:#}")))?;
    }
    let files: Vec<_> = std::fs::read_dir(out)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|x| x == "png")
                && !p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .contains("preview")
        })
        .collect();
    if files.len() != 1 {
        return Err(error("Expected one output PNG".into()));
    }
    Raster::open(&files[0])
}
