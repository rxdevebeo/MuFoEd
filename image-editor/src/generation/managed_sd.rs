//! Staged Plakat SD adapter with a bounded-attention `UNet` bridge, pinned to `PLAKAT_REFERENCE`.
//! Uses Plakat models, `LoRA` merging, prompt weighting, masks, scheduler and preprocessing.
//! CPU tensors are loaded once per fingerprint; each GPU stage is destroyed and synchronized
//! before the next component is constructed. This trades `PCIe` transfer for a bounded peak.
use super::{plakat_unet::Model as SdUNet, Mode, Request};
use crate::{profiling, Raster};
use anyhow::{anyhow, bail, Result};
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::stable_diffusion::{
    self,
    vae::{AutoEncoderKL, AutoEncoderKLConfig},
};
use mufoed_model_runtime::{
    residency::{Pinned, Resource},
    Lease, Memory, Phase,
};
use plakat::pipelines::{
    lora::{MergeTarget, ResolvedLora},
    sd_core::SdVariant,
    sdxl_unet::SdxlAddEmbedConfig,
};
use std::fmt::Write as _;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};
use tokenizers::Tokenizer;

struct Weights {
    tensors: Mutex<Option<HashMap<String, Tensor>>>,
    bytes: u64,
    alive: AtomicBool,
}
impl Resource for Weights {
    fn memory(&self) -> Memory {
        Memory {
            ram: Some(if self.alive.load(Ordering::Acquire) {
                self.bytes
            } else {
                0
            }),
            vram: Some(0),
        }
    }
    fn offload(&self) -> std::result::Result<bool, String> {
        Ok(false)
    }
    fn unload(&self) -> std::result::Result<(), String> {
        self.tensors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        self.alive.store(false, Ordering::Release);
        Ok(())
    }
}
impl Weights {
    fn prepared(tensors: HashMap<String, Tensor>) -> Self {
        let bytes = tensors
            .values()
            .map(|t| (t.elem_count() * t.dtype().size_in_bytes()) as u64)
            .sum();
        Self {
            tensors: Mutex::new(Some(tensors)),
            bytes,
            alive: AtomicBool::new(true),
        }
    }
    fn load(path: &Path, dtype: DType) -> candle_core::Result<Self> {
        let _timing = profiling::scope("weights.prepare_cpu_dtype");
        let raw = candle_core::safetensors::load(path, &Device::Cpu)?;
        let tensors = raw
            .into_iter()
            .map(|(name, tensor)| Ok((name, tensor.to_dtype(dtype)?)))
            .collect::<candle_core::Result<HashMap<_, _>>>()?;
        let weights = Self::prepared(tensors);
        eprintln!(
            "[resident] prepared CPU weight load: {} ({} bytes, {dtype:?})",
            path.display(),
            weights.bytes
        );
        Ok(weights)
    }
    fn builder(&self, device: &Device, dtype: DType) -> Result<VarBuilder<'static>> {
        let _timing = profiling::gpu("weights.cpu_to_device", device)?;
        let map = self
            .tensors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let tensors = map
            .as_ref()
            .ok_or_else(|| anyhow!("evicted pinned weights"))?
            .iter()
            .map(|(k, v)| {
                if v.dtype() != dtype {
                    candle_core::bail!("prepared weight dtype mismatch");
                }
                Ok((k.clone(), v.to_device(device)?))
            })
            .collect::<candle_core::Result<HashMap<_, _>>>()?;
        Ok(VarBuilder::from_tensors(tensors, dtype, device))
    }
}
fn fingerprint(path: &Path) -> Result<String> {
    mufoed_model_runtime::fingerprint::file(path).map_err(|e| anyhow!(e))
}

struct Files {
    l: PathBuf,
    g: Option<PathBuf>,
    unet: PathBuf,
    vae: PathBuf,
    scheduler: Option<super::dpmpp::Config>,
    tokenizer_identity: String,
    tok_l: Tokenizer,
    tok_g: Option<Tokenizer>,
    variant: SdVariant,
    loras: Vec<ResolvedLora>,
    signature: String,
}
fn resolve_loras(request: &Request, variant: SdVariant) -> Result<(Vec<ResolvedLora>, String)> {
    let mut loras = Vec::new();
    let mut signature = format!("{}:merge-device-v1:{variant:?}", super::PLAKAT_REFERENCE);
    for item in request.loras.iter().filter(|l| l.enabled) {
        write!(
            signature,
            ":{}:{:08x}",
            fingerprint(&item.path)?,
            item.weight.to_bits()
        )?;
        loras.push(ResolvedLora {
            path: item.path.clone(),
            scale: item.weight,
            display: item.path.to_string_lossy().into_owned(),
        });
    }
    Ok((loras, signature))
}
async fn first(repo: &str, names: &[&str]) -> Result<PathBuf> {
    let options: Vec<_> = names.iter().map(|name| (repo, *name)).collect();
    plakat::hf::download::get_first_of(&options).await
}
async fn resolve_vae(request: &Request, repo: &str) -> Result<PathBuf> {
    if let Some(path) = &request.vae {
        return Ok(path.clone());
    }
    // Default means this checkpoint's VAE on both CPU and GPU. A global alias
    // may have been populated by an older converter and is never consulted.
    let names = [
        "vae/diffusion_pytorch_model.safetensors",
        "vae/diffusion_pytorch_model.fp16.safetensors",
        "vae/weights.safetensors",
    ];
    if let Some(path) = cached_vae(&plakat::hf::cache::hf_cache_root(), repo, &names) {
        return Ok(path);
    }
    first(repo, &names).await
}
// Check all cached variants before trying a download of the preferred filename.
fn cached_vae(root: &Path, repo: &str, names: &[&str]) -> Option<PathBuf> {
    if repo
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == ".." || part.contains('\\'))
    {
        return None;
    }
    let folder = root.join(format!("models--{}", repo.replace('/', "--")));
    let revision = std::fs::read_to_string(folder.join("refs/main")).ok()?;
    let revision = revision.trim();
    if revision.is_empty() || revision.contains(['/', '\\']) || revision == "." || revision == ".."
    {
        return None;
    }
    let snapshot = folder.join("snapshots").join(revision);
    names
        .iter()
        .map(|name| snapshot.join(name))
        .find(|path| path.is_file())
}
impl Files {
    async fn resolve(request: &Request) -> Result<Self> {
        let repo = if request.model.contains('/') {
            request.model.clone()
        } else {
            plakat::hf::resolve_alias(&request.model).to_owned()
        };
        let variant = match request.family {
            Some(crate::assets::Family::Sdxl) => SdVariant::Sdxl,
            Some(crate::assets::Family::Sd15) => SdVariant::Sd15,
            _ => SdVariant::detect(&repo),
        };
        if repo.to_lowercase().contains("flux") {
            bail!("Managed staged adapter supports SD 1.5/2.1/SDXL only");
        }
        let tok_l = first(&repo, &["tokenizer/tokenizer.json"]).await?;
        let tok_g = if variant.is_xl() {
            Some(first(&repo, &["tokenizer_2/tokenizer.json"]).await?)
        } else {
            None
        };
        let l = first(
            &repo,
            &[
                "text_encoder/model.fp16.safetensors",
                "text_encoder/model.safetensors",
            ],
        )
        .await?;
        let g = if variant.is_xl() {
            Some(
                first(
                    &repo,
                    &[
                        "text_encoder_2/model.fp16.safetensors",
                        "text_encoder_2/model.safetensors",
                    ],
                )
                .await?,
            )
        } else {
            None
        };
        let unet = first(
            &repo,
            &[
                "unet/diffusion_pytorch_model.fp16.safetensors",
                "unet/diffusion_pytorch_model.safetensors",
            ],
        )
        .await?;
        let vae = resolve_vae(request, &repo).await?;
        let scheduler = if request.sampler == super::Sampler::Dpmpp2mKarras {
            let path = first(&repo, &["scheduler/scheduler_config.json"]).await?;
            let config: super::dpmpp::Config = serde_json::from_reader(std::fs::File::open(path)?)?;
            // Refuse unsupported prediction/schedule before loading model weights onto GPU.
            super::dpmpp::Dpmpp::new(&config, request.steps as usize, 0)?;
            Some(config)
        } else {
            None
        };
        let (loras, signature) = resolve_loras(request, variant)?;
        Ok(Self {
            l,
            g,
            unet,
            vae,
            scheduler,
            tokenizer_identity: tokenizer_identity(&tok_l, tok_g.as_deref())?,
            tok_l: Tokenizer::from_file(tok_l).map_err(|e| anyhow!(e.to_string()))?,
            tok_g: tok_g
                .map(Tokenizer::from_file)
                .transpose()
                .map_err(|e| anyhow!(e.to_string()))?,
            variant,
            loras,
            signature,
        })
    }
    fn key(
        &self,
        path: &Path,
        target: Option<&MergeTarget>,
        dtype: DType,
        uuid: &str,
    ) -> Result<String> {
        let key = format!(
            "plakat:prepared-v2:{:?}:{}:{uuid}:{}:{}",
            dtype,
            target.map_or("vae", |t| t.name),
            if target.is_some() {
                self.signature.clone()
            } else {
                format!("{}:{:?}", super::PLAKAT_REFERENCE, self.variant)
            },
            fingerprint(path)?
        );
        Ok(key)
    }
    fn weights<'lease>(
        &self,
        path: &Path,
        target: Option<MergeTarget>,
        lease: &'lease Lease,
        dtype: DType,
        uuid: &str,
        merge_device: &Device,
    ) -> Result<Pinned<'lease, Weights>> {
        let _lookup = profiling::scope("weights.lookup_and_load");
        let key = self.key(path, target.as_ref(), dtype, uuid)?;
        let resource = lease
            .cached(&key, || {
                let _cold = profiling::scope("weights.cold_load");
                let merged = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
                let effective = if let Some(target) = target {
                    if self.loras.is_empty() {
                        path
                    } else {
                        let backend = if matches!(merge_device, Device::Cpu) {
                            "CPU"
                        } else {
                            "GPU"
                        };
                        lease.stage(
                            Phase::Loading,
                            &format!("LoRA · {} · объединение на {backend}", target.name),
                        );
                        let _merge = profiling::gpu("weights.lora_merge", merge_device)
                            .map_err(|e| e.to_string())?;
                        let started = std::time::Instant::now();
                        let (modified, total) = plakat::pipelines::lora::merge_loras_into_weights(
                            path,
                            merged.path(),
                            &self.loras,
                            1.0,
                            merge_device,
                            target,
                        )
                        .map_err(|e| e.to_string())?;
                        merge_device.synchronize().map_err(|e| e.to_string())?;
                        if total > 0 && modified == 0 {
                            return Err(format!("LoRA did not match any {} weights", target.name));
                        }
                        eprintln!(
                            "[lora] {} on {backend}: {modified}/{total} targets in {:.1}s",
                            target.name,
                            started.elapsed().as_secs_f32()
                        );
                        lease.stage(
                            Phase::Loading,
                            &format!(
                                "LoRA · {} · {modified}/{total} применений готовы",
                                target.name
                            ),
                        );
                        merged.path()
                    }
                } else {
                    path
                };
                Weights::load(effective, dtype).map_err(|e| e.to_string())
            })
            .map_err(|e| anyhow!(e))?;
        Ok(resource)
    }
}
fn check(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        bail!("cancelled");
    }
    Ok(())
}
fn apply_weights(hidden: Tensor, weights: Option<Tensor>) -> Result<Tensor> {
    Ok(if let Some(weights) = weights {
        hidden.broadcast_mul(&weights.to_dtype(hidden.dtype())?)?
    } else {
        hidden
    })
}
fn vae_dtype(variant: SdVariant, denoiser: DType) -> DType {
    if variant.is_xl() {
        DType::F32
    } else {
        denoiser
    }
}
fn vae<'lease>(
    files: &Files,
    lease: &'lease Lease,
    device: &Device,
    dtype: DType,
    uuid: &str,
) -> Result<(AutoEncoderKL, Pinned<'lease, Weights>)> {
    let _timing = profiling::gpu("vae.model_prepare", device)?;
    let weights = files.weights(&files.vae, None, lease, dtype, uuid, device)?;
    let model = AutoEncoderKL::new(
        weights.builder(device, dtype)?,
        3,
        3,
        AutoEncoderKLConfig {
            block_out_channels: vec![128, 256, 512, 512],
            layers_per_block: 2,
            latent_channels: 4,
            norm_num_groups: 32,
            use_quant_conv: true,
            use_post_quant_conv: true,
        },
    )?;
    Ok((model, weights))
}

fn tokenizer_identity(l: &Path, g: Option<&Path>) -> Result<String> {
    Ok(format!(
        "{}:{:?}",
        fingerprint(l)?,
        g.map(fingerprint).transpose()?
    ))
}
fn conditioning_key(files: &Files, request: &Request, dtype: DType, uuid: &str) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(format!(
        "clip-context-v1:{}:{:?}:{dtype:?}:{uuid}",
        super::PLAKAT_REFERENCE,
        files.variant
    ));
    for value in [request.prompt.as_str(), request.negative.as_str()] {
        hash.update(value.len().to_le_bytes());
        hash.update(value.as_bytes());
    }
    hash.update([u8::from(request.guidance > 1.0)]);
    hash.update(
        files
            .key(
                &files.l,
                Some(&if files.variant.is_xl() {
                    MergeTarget::TE1_SDXL
                } else {
                    MergeTarget::TE_SD15
                }),
                dtype,
                uuid,
            )?
            .as_bytes(),
    );
    if let Some(path) = &files.g {
        hash.update(
            files
                .key(path, Some(&MergeTarget::TE2_SDXL), dtype, uuid)?
                .as_bytes(),
        );
    }
    hash.update(files.tokenizer_identity.as_bytes());
    Ok(format!("clip-context-v1:{:x}", hash.finalize()))
}
struct Conditioning {
    hidden: Tensor,
    pooled: Option<Tensor>,
}
struct Init {
    base: Option<Tensor>,
    masked: Option<Tensor>,
    mask: Tensor,
    inverse: Tensor,
}
struct Staged<'a> {
    files: Files,
    device: Device,
    dtype: DType,
    uuid: String,
    lease: &'a Lease,
    cfg: stable_diffusion::StableDiffusionConfig,
    unet_weights: Pinned<'a, Weights>,
    channels: usize,
}
struct Synchronize(Device);
impl Drop for Synchronize {
    fn drop(&mut self) {
        let _ = self.0.synchronize();
    }
}
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)] // Validated dimensions <= 16M pixels, steps <= 150, training timesteps <= 1000.
impl Staged<'_> {
    fn encode_l(&self, texts: &[Vec<super::prompt_tokens::Chunk>]) -> Result<Tensor> {
        let _total = profiling::gpu("clip_l.total", &self.device)?;
        self.lease.stage(Phase::Loading, "CLIP-L device placement");
        let target = if self.files.variant.is_xl() {
            MergeTarget::TE1_SDXL
        } else {
            MergeTarget::TE_SD15
        };
        let weights = self.files.weights(
            &self.files.l,
            Some(target),
            self.lease,
            self.dtype,
            &self.uuid,
            &self.device,
        )?;
        let cfg = match self.files.variant {
            SdVariant::Sd15 => plakat::pipelines::vendored_clip::Config::v1_5(),
            SdVariant::Sd21 => plakat::pipelines::vendored_clip::Config::v2_1(),
            SdVariant::Sdxl => plakat::pipelines::vendored_clip::Config::sdxl(),
        };
        let model = plakat::pipelines::vendored_clip::ClipTextTransformer::new(
            weights.builder(&self.device, self.dtype)?,
            &cfg,
        )?;
        let _forward = profiling::gpu("clip_l.forward", &self.device)?;
        self.lease.stage(Phase::Running, "CLIP-L encode");
        let mut branches = Vec::new();
        for chunks in texts {
            let mut outputs = Vec::new();
            for text in chunks {
                let (ids, weights) = text.tensors(&self.device, self.dtype)?;
                let (last, hidden) = model.forward_until_encoder_layer(
                    &ids,
                    usize::MAX,
                    if self.files.variant.is_xl() { -2 } else { -1 },
                )?;
                outputs.push(apply_weights(
                    if self.files.variant.is_xl() {
                        hidden
                    } else {
                        last
                    },
                    weights,
                )?);
            }
            branches.push(Tensor::cat(&outputs, 1)?);
        }
        Ok(Tensor::cat(&branches, 0)?)
    }
    fn encode_g(&self, texts: &[Vec<super::prompt_tokens::Chunk>]) -> Result<(Tensor, Tensor)> {
        let _total = profiling::gpu("clip_g.total", &self.device)?;
        self.lease.stage(Phase::Loading, "CLIP-G device placement");
        let weights = self.files.weights(
            self.files.g.as_ref().expect("XL encoder"),
            Some(MergeTarget::TE2_SDXL),
            self.lease,
            self.dtype,
            &self.uuid,
            &self.device,
        )?;
        let model = plakat::pipelines::sdxl_clip::SdxlClipGTextTransformer::new(
            weights.builder(&self.device, self.dtype)?,
            &plakat::pipelines::vendored_clip::Config::sdxl2(),
            1280,
        )?;
        let _forward = profiling::gpu("clip_g.forward", &self.device)?;
        self.lease.stage(Phase::Running, "CLIP-G encode");
        let mut branches = Vec::new();
        let mut pools = Vec::new();
        for chunks in texts {
            let mut outputs = Vec::new();
            for (index, text) in chunks.iter().enumerate() {
                let (ids, weights) = text.tensors(&self.device, self.dtype)?;
                let (hidden, pooled) = model.forward_for_sdxl(&ids)?;
                outputs.push(apply_weights(hidden, weights)?);
                if index == 0 {
                    pools.push(pooled);
                }
            }
            branches.push(Tensor::cat(&outputs, 1)?);
        }
        Ok((Tensor::cat(&branches, 0)?, Tensor::cat(&pools, 0)?))
    }
    fn encode(&self, request: &Request, cancel: &AtomicBool) -> Result<Conditioning> {
        let _timing = profiling::scope("conditioning.total");
        let key = conditioning_key(&self.files, request, self.dtype, &self.uuid)?;
        let resource = self
            .lease
            .cached(&key, || {
                let _compute = profiling::scope("conditioning.compute");
                let value = self
                    .encode_uncached(request, cancel)
                    .map_err(|e| e.to_string())?;
                let mut tensors = HashMap::from([(
                    "hidden".into(),
                    value
                        .hidden
                        .to_device(&Device::Cpu)
                        .map_err(|e| e.to_string())?,
                )]);
                if let Some(pooled) = value.pooled {
                    tensors.insert(
                        "pooled".into(),
                        pooled.to_device(&Device::Cpu).map_err(|e| e.to_string())?,
                    );
                }
                Ok(Weights::prepared(tensors))
            })
            .map_err(|e| anyhow!(e))?;
        check(cancel)?;
        let _upload = profiling::gpu("conditioning.upload", &self.device)?;
        let guard = resource
            .tensors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let tensors = guard
            .as_ref()
            .ok_or_else(|| anyhow!("evicted pinned conditioning"))?;
        Ok(Conditioning {
            hidden: tensors
                .get("hidden")
                .ok_or_else(|| anyhow!("missing conditioning"))?
                .to_device(&self.device)?,
            pooled: tensors
                .get("pooled")
                .map(|v| v.to_device(&self.device))
                .transpose()?,
        })
    }
    fn encode_uncached(&self, request: &Request, cancel: &AtomicBool) -> Result<Conditioning> {
        let mut texts = vec![];
        if request.guidance > 1.0 {
            texts.push(request.negative.as_str());
        }
        texts.push(request.prompt.as_str());
        let token_timing = profiling::scope("clip.tokenize");
        let batch = super::prompt_tokens::prepare(
            (&self.files.tok_l, &self.cfg.clip),
            self.files.tok_g.as_ref().zip(self.cfg.clip2.as_ref()),
            &texts,
        )?;
        drop(token_timing);
        let hidden_l = self.encode_l(&batch.l)?;
        self.lease.stage(
            Phase::Offloading,
            "CLIP-L device model released; CPU cache retained",
        );
        self.device.synchronize()?;
        check(cancel)?;
        let value = if self.files.variant.is_xl() {
            let (hidden_g, pooled) = self.encode_g(batch.g.as_ref().expect("XL token batch"))?;
            Conditioning {
                hidden: Tensor::cat(&[hidden_l, hidden_g], 2)?,
                pooled: Some(pooled),
            }
        } else {
            Conditioning {
                hidden: hidden_l,
                pooled: None,
            }
        };
        self.device.synchronize()?;
        check(cancel)?;
        Ok(value)
    }
    fn init(&self, request: &Request, input: &Path, cancel: &AtomicBool) -> Result<Init> {
        let _total = profiling::gpu("vae.encode_total", &self.device)?;
        let (width, height) = (self.cfg.width as u32, self.cfg.height as u32);
        let mut pixels_mask = plakat::imaging::mask::Mask::solid_one(width, height);
        if request.mode == Mode::Inpaint {
            pixels_mask =
                plakat::imaging::mask::Mask::load(&input.join("mask.png"), width, height)?;
            if request.mask_invert {
                pixels_mask.invert();
            }
            pixels_mask.feather(request.mask_feather);
        }
        let mask = pixels_mask.to_latent_tensor(&self.device, self.dtype)?;
        let inverse = (mask.ones_like()? - &mask)?;
        if request.mode == Mode::TextToImage {
            if self.channels == 9 {
                bail!("A 9-channel inpaint model requires an input image");
            }
            return Ok(Init {
                base: None,
                masked: None,
                mask,
                inverse,
            });
        }
        self.lease.stage(
            Phase::Loading,
            if super::vae_tiles::needed([width, height]) {
                "VAE encode · tiled 512px"
            } else {
                "VAE encode"
            },
        );
        let vae_dtype = vae_dtype(self.files.variant, self.dtype);
        let (model, _weights) = vae(&self.files, self.lease, &self.device, vae_dtype, &self.uuid)?;
        let pixels = plakat::imaging::preprocess::sd_image_tensor(
            &input.join("input.png"),
            width,
            height,
            &self.device,
            vae_dtype,
        )?;
        let _ = self.device.set_seed(plakat::pipelines::seeds::prepare_seed(
            request.seed,
            &self.device,
        ));
        let masked = if self.channels == 9 {
            let keep: Vec<f32> = pixels_mask
                .pixels
                .iter()
                .map(|m| if *m < 0.5 { 1.0 } else { 0.0 })
                .collect();
            let keep =
                Tensor::from_vec(keep, (1, 1, height as usize, width as usize), &self.device)?
                    .to_dtype(vae_dtype)?;
            Some(
                (super::vae_tiles::encode(&model, &pixels.broadcast_mul(&keep)?, cancel)?
                    * self.files.variant.vae_scale())?
                .to_dtype(self.dtype)?,
            )
        } else {
            None
        };
        let _ = self.device.set_seed(plakat::pipelines::seeds::prepare_seed(
            request.seed,
            &self.device,
        ));
        let base = Some(
            (super::vae_tiles::encode(&model, &pixels, cancel)? * self.files.variant.vae_scale())?
                .to_dtype(self.dtype)?,
        );
        Ok(Init {
            base,
            masked,
            mask,
            inverse,
        })
    }
    fn unet(&self) -> Result<SdUNet> {
        let _timing = profiling::gpu("unet.model_prepare", &self.device)?;
        self.lease.stage(Phase::Loading, "UNet device placement");
        let mut cfg = if self.files.variant.is_xl() {
            plakat::pipelines::controlnet::sdxl_unet_config()
        } else {
            plakat::pipelines::controlnet::sd15_unet_config()
        };
        if self.files.variant == SdVariant::Sd21 {
            cfg.cross_attention_dim = 1024;
            cfg.use_linear_projection = true;
            for (block, heads) in cfg.blocks.iter_mut().zip([5, 10, 20, 20]) {
                block.attention_head_dim = heads;
            }
        }
        if super::plakat_unet::bounded_attention(
            [self.cfg.width as u32, self.cfg.height as u32],
            self.files.variant.is_xl(),
        ) {
            cfg.sliced_attention_size = Some(2);
        }
        let model = SdUNet::new(
            self.unet_weights.builder(&self.device, self.dtype)?,
            self.channels,
            cfg,
            self.files.variant.is_xl().then(SdxlAddEmbedConfig::base),
        )?;
        Ok(model)
    }
    fn scheduler(
        &self,
        request: &Request,
        has_source: bool,
    ) -> Result<Box<dyn stable_diffusion::schedulers::Scheduler>> {
        let begin = if has_source {
            (((1.0 - request.strength) * request.steps as f32).round() as usize)
                .min(request.steps.saturating_sub(1) as usize)
        } else {
            0
        };
        let scheduler: Box<dyn stable_diffusion::schedulers::Scheduler> = match request.sampler {
            super::Sampler::Ddim => plakat::pipelines::scheduler::build(
                plakat::pipelines::scheduler::SchedulerKind::default(),
                &self.cfg,
                request.steps as usize,
            )?,
            super::Sampler::Dpmpp2mKarras => Box::new(super::dpmpp::Dpmpp::new(
                self.files.scheduler.as_ref().expect("DPM++ config"),
                request.steps as usize,
                begin,
            )?),
        };
        Ok(scheduler)
    }
    fn prediction(
        &self,
        model: &SdUNet,
        sample: &Tensor,
        time: usize,
        conditioning: &Conditioning,
        time_ids: Option<&Tensor>,
    ) -> Result<Tensor> {
        // Keep eager attention's quadratic workspace to one CFG branch at a time.
        // Each branch still sees the full image and its complete prompt context.
        let sequential = super::plakat_unet::bounded_attention(
            [self.cfg.width as u32, self.cfg.height as u32],
            self.files.variant.is_xl(),
        ) && sample.dim(0)? == 2;
        if !sequential {
            return Ok(model.forward(
                sample,
                time as f64,
                &conditioning.hidden,
                conditioning.pooled.as_ref(),
                time_ids,
            )?);
        }
        let mut predictions = Vec::with_capacity(2);
        for branch in 0..2 {
            let hidden = conditioning.hidden.narrow(0, branch, 1)?.contiguous()?;
            let pooled = conditioning
                .pooled
                .as_ref()
                .map(|v| v.narrow(0, branch, 1)?.contiguous())
                .transpose()?;
            let ids = time_ids
                .map(|v| v.narrow(0, branch, 1)?.contiguous())
                .transpose()?;
            predictions.push(model.forward(
                &sample.narrow(0, branch, 1)?.contiguous()?,
                time as f64,
                &hidden,
                pooled.as_ref(),
                ids.as_ref(),
            )?);
            self.device.synchronize()?;
        }
        Ok(Tensor::cat(&predictions, 0)?)
    }
    fn denoise(
        &self,
        request: &Request,
        conditioning: &Conditioning,
        init: &Init,
        cancel: &AtomicBool,
    ) -> Result<Tensor> {
        let do_cfg = request.guidance > 1.0;
        let mut scheduler = self.scheduler(request, init.base.is_some())?;
        let times = scheduler.timesteps().to_vec();
        let start = if init.base.is_some() {
            (((1.0 - request.strength) * times.len() as f32).round() as usize)
                .min(times.len().saturating_sub(1))
        } else {
            0
        };
        let _ = self.device.set_seed(plakat::pipelines::seeds::prepare_seed(
            request.seed,
            &self.device,
        ));
        let mut latents = if let Some(base) = &init.base {
            let noise =
                Tensor::randn(0f32, 1f32, base.shape(), &self.device)?.to_dtype(self.dtype)?;
            let noised = scheduler.add_noise(base, noise.clone(), times[start])?;
            if self.channels == 9 {
                (scheduler
                    .add_noise(&base.zeros_like()?, noise, times[start])?
                    .broadcast_mul(&init.mask)?
                    + noised.broadcast_mul(&init.inverse)?)?
            } else {
                noised
            }
        } else {
            (Tensor::randn(
                0f32,
                1f32,
                (1, 4, self.cfg.height / 8, self.cfg.width / 8),
                &self.device,
            )?
            .to_dtype(self.dtype)?
                * scheduler.init_noise_sigma())?
        };
        let time_ids = if self.files.variant.is_xl() {
            let ids = plakat::pipelines::sdxl_unet::build_add_time_ids_base(
                self.cfg.height as u32,
                self.cfg.width as u32,
                &self.device,
                self.dtype,
            )?;
            Some(if do_cfg {
                Tensor::cat(&[&ids, &ids], 0)?
            } else {
                ids
            })
        } else {
            None
        };
        let model = self.unet()?;
        self.lease.stage(
            Phase::Running,
            if super::plakat_unet::bounded_attention(
                [self.cfg.width as u32, self.cfg.height as u32],
                self.files.variant.is_xl(),
            ) && do_cfg
            {
                "UNet denoise · sequential CFG"
            } else {
                "UNet denoise"
            },
        );
        for &time in &times[start..] {
            let _step = profiling::gpu("unet.step", &self.device)?;
            check(cancel)?;
            if self.channels == 4 {
                if let Some(base) = &init.base {
                    let noise = Tensor::randn(0f32, 1f32, base.shape(), &self.device)?
                        .to_dtype(self.dtype)?;
                    let noised = scheduler.add_noise(base, noise, time)?;
                    latents = (latents.broadcast_mul(&init.mask)?
                        + noised.broadcast_mul(&init.inverse)?)?;
                }
            }
            let batch = |tensor: &Tensor| -> candle_core::Result<Tensor> {
                if do_cfg {
                    Tensor::cat(&[tensor, tensor], 0)
                } else {
                    Ok(tensor.clone())
                }
            };
            let mut sample = scheduler.scale_model_input(batch(&latents)?, time)?;
            if let Some(masked) = &init.masked {
                sample = Tensor::cat(&[sample, batch(&init.mask)?, batch(masked)?], 1)?;
            }
            let prediction =
                self.prediction(&model, &sample, time, conditioning, time_ids.as_ref())?;
            let prediction = if do_cfg {
                let chunks = prediction.chunk(2, 0)?;
                (&chunks[0] + ((&chunks[1] - &chunks[0])? * f64::from(request.guidance))?)?
            } else {
                prediction
            };
            latents = scheduler.step(&prediction, time, &latents)?;
        }
        if let Some(base) = &init.base {
            latents = (latents.broadcast_mul(&init.mask)? + base.broadcast_mul(&init.inverse)?)?;
        }
        Ok(latents)
    }
    fn decode(&self, latents: &Tensor, cancel: &AtomicBool) -> Result<Raster> {
        let _total = profiling::gpu("vae.decode_total", &self.device)?;
        let vae_dtype = vae_dtype(self.files.variant, self.dtype);
        let latents = latents.to_dtype(vae_dtype)?;
        if !latents
            .to_dtype(DType::F32)?
            .sum_all()?
            .to_scalar::<f32>()?
            .is_finite()
        {
            bail!("Denoiser produced non-finite latents before VAE decoding");
        }
        self.lease.stage(
            Phase::Loading,
            if super::vae_tiles::needed([self.cfg.width as u32, self.cfg.height as u32]) {
                "VAE decode · tiled 512px"
            } else {
                "VAE decode"
            },
        );
        let (model, _weights) = vae(&self.files, self.lease, &self.device, vae_dtype, &self.uuid)?;
        #[cfg(feature = "plakat-cuda")]
        let memory_watch = profiling::watch_memory("vae.decode_memory", &self.device);
        let decode_timing = profiling::gpu("vae.decode_compute", &self.device)?;
        let image =
            super::vae_tiles::decode(&model, &(latents / self.files.variant.vae_scale())?, cancel)?;
        drop(decode_timing);
        #[cfg(feature = "plakat-cuda")]
        drop(memory_watch);
        let _raster = profiling::gpu("vae.decode_to_raster", &self.device)?;
        if !image
            .to_dtype(DType::F32)?
            .sum_all()?
            .to_scalar::<f32>()?
            .is_finite()
        {
            bail!("VAE produced non-finite values; check VAE weights and model compatibility");
        }
        let image = (((image / 2.0)? + 0.5)?.clamp(0f32, 1f32)? * 255.0)?
            .to_dtype(DType::U8)?
            .i(0)?
            .permute((1, 2, 0))?
            .to_device(&Device::Cpu)?
            .flatten_all()?
            .to_vec1::<u8>()?;
        Ok(Raster::new(
            self.cfg.width as u32,
            self.cfg.height as u32,
            image
                .chunks_exact(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
        )?)
    }
}
fn execution_device(gpu: bool, uuid: &str) -> Result<Device> {
    // Context/compiled kernels are process infrastructure, never cached model tensors.
    // The bounded worker owns execution. Runtime measures actual free VRAM including
    // this context overhead before admitting every task; model weights remain evictable.
    static DEVICES: std::sync::OnceLock<Mutex<HashMap<String, Device>>> =
        std::sync::OnceLock::new();
    if !gpu {
        return Ok(Device::Cpu);
    }
    let mut devices = DEVICES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(device) = devices.get(uuid) {
        return Ok(device.clone());
    }
    let device = plakat::api::device("auto")?;
    if matches!(device, Device::Cpu) {
        bail!("CUDA initialization failed; select CPU explicitly");
    }
    devices.insert(uuid.to_owned(), device.clone());
    Ok(device)
}
fn unet_channels(weights: &Weights) -> Result<usize> {
    let channels = weights
        .tensors
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .and_then(|m| m.get("conv_in.weight"))
        .ok_or_else(|| anyhow!("UNet missing conv_in.weight"))?
        .dims4()?
        .1;
    if !matches!(channels, 4 | 9) {
        bail!("Unsupported UNet input channels: {channels}");
    }
    Ok(channels)
}
/// Transactional staged inference, with cancellation between components and denoise steps.
pub(super) async fn run(
    request: &Request,
    source: &Raster,
    input: &Path,
    lease: &Lease,
    cancel: &AtomicBool,
) -> Result<Raster> {
    let gpu = cfg!(feature = "plakat-cuda");
    let dtype = if gpu { DType::F16 } else { DType::F32 };
    let uuid = lease.device_identity(if gpu { "auto" } else { "cpu" })?;
    let [width, height] = if request.mode == Mode::TextToImage {
        request
            .output_size
            .unwrap_or([source.width(), source.height()])
    } else {
        [source.width(), source.height()]
    };
    check(cancel)?;
    let _total = profiling::scope(format!("native.{:?}.{width}x{height}", request.mode));
    let resolve_timing = profiling::scope("native.resolve_and_keys");
    let files = Files::resolve(request).await?;
    let cfg = files.variant.config(width as usize, height as usize);
    let mut keys = vec![
        files.key(&files.unet, Some(&MergeTarget::UNET), dtype, &uuid)?,
        files.key(
            &files.l,
            Some(&if files.variant.is_xl() {
                MergeTarget::TE1_SDXL
            } else {
                MergeTarget::TE_SD15
            }),
            dtype,
            &uuid,
        )?,
        files.key(&files.vae, None, vae_dtype(files.variant, dtype), &uuid)?,
    ];
    if let Some(g) = &files.g {
        keys.push(files.key(g, Some(&MergeTarget::TE2_SDXL), dtype, &uuid)?);
    }
    drop(resolve_timing);
    let admission_timing = profiling::scope("native.admission");
    // Use resolved architecture, including opaque local aliases, for admission.
    let mut estimation = request.clone();
    estimation.family = Some(if files.variant.is_xl() {
        crate::assets::Family::Sdxl
    } else {
        crate::assets::Family::Sd15
    });
    lease
        .prepare_reusing(
            super::native::estimate(&estimation, [width, height]),
            &uuid,
            &keys,
        )
        .map_err(|e| anyhow!("{}x{} {:?} admission: {e}", width, height, files.variant))?;
    check(cancel)?;
    drop(admission_timing);
    let device_timing = profiling::scope("native.device_init");
    let device = execution_device(gpu, &uuid)?;
    drop(device_timing);
    if gpu && matches!(device, Device::Cpu) {
        bail!("CUDA initialization failed; CPU execution must be selected explicitly");
    }
    let _sync = Synchronize(device.clone());
    let weights = files.weights(
        &files.unet,
        Some(MergeTarget::UNET),
        lease,
        dtype,
        &uuid,
        &device,
    )?;
    let channels = unet_channels(&weights)?;
    let staged = Staged {
        files,
        device,
        dtype,
        uuid,
        lease,
        cfg,
        unet_weights: weights,
        channels,
    };
    let conditioning = staged.encode(request, cancel)?;
    let init = staged.init(request, input, cancel)?;
    if init.base.is_some() && !matches!(staged.device, Device::Cpu) {
        lease.stage(Phase::Offloading, "VAE encoder GPU released");
    }
    staged.device.synchronize()?;
    check(cancel)?;
    #[cfg(feature = "plakat-cuda")]
    let memory_watch = profiling::watch_memory("unet.memory", &staged.device);
    let denoise_timing = profiling::gpu("unet.total", &staged.device)?;
    let latents = staged.denoise(request, &conditioning, &init, cancel)?;
    drop(denoise_timing);
    #[cfg(feature = "plakat-cuda")]
    drop(memory_watch);
    staged.device.synchronize()?;
    check(cancel)?;
    lease.stage(
        Phase::Offloading,
        "UNet device model released; CPU cache retained",
    );
    let raster = staged.decode(&latents, cancel)?;
    staged.device.synchronize()?;
    Ok(raster)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prepared_weights_match_legacy_cast_account_bytes_and_release() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("weights.safetensors");
        let original = Tensor::from_vec(
            vec![-2.0f32, -0.125, 0.0, 0.3333, 1.0, 100.0],
            (2, 3),
            &Device::Cpu,
        )?
        .to_dtype(DType::F16)?;
        candle_core::safetensors::save(
            &HashMap::from([("weight".to_owned(), original.clone())]),
            &path,
        )?;
        let weights = Weights::load(&path, DType::F32)?;
        assert_eq!(weights.memory().ram, Some(24));
        assert_eq!(weights.memory().vram, Some(0));
        let builder = weights.builder(&Device::Cpu, DType::F32)?;
        let actual = builder.get((2, 3), "weight")?;
        assert_eq!(
            actual.to_vec2::<f32>()?,
            original.to_dtype(DType::F32)?.to_vec2::<f32>()?
        );
        assert!(weights.builder(&Device::Cpu, DType::F16).is_err());
        drop(builder);
        weights.unload().map_err(|e| anyhow!(e))?;
        assert_eq!(weights.memory().ram, Some(0));
        assert!(weights.builder(&Device::Cpu, DType::F32).is_err());
        Ok(())
    }
    #[test]
    fn conditioning_cache_keys_cover_inputs_and_ignore_sampling_only_parameters() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let encoder = directory.path().join("encoder");
        std::fs::write(&encoder, b"encoder-v1")?;
        let mut files = Files {
            l: encoder.clone(),
            g: Some(encoder.clone()),
            unet: encoder.clone(),
            vae: encoder,
            scheduler: None,
            tokenizer_identity: "tokenizers-v1".into(),
            tok_l: Tokenizer::new(tokenizers::models::wordlevel::WordLevel::default()),
            tok_g: None,
            variant: SdVariant::Sdxl,
            loras: Vec::new(),
            signature: "encoder-lora-v1".into(),
        };
        let request = Request::default();
        let key = conditioning_key(&files, &request, DType::F16, "gpu-a")?;
        assert_eq!(
            key,
            conditioning_key(&files, &request, DType::F16, "gpu-a")?
        );
        let mut changed = request.clone();
        changed.steps += 1;
        changed.seed += 1;
        changed.output_size = Some([1536, 1536]);
        changed.guidance = 6.0;
        assert_eq!(
            key,
            conditioning_key(&files, &changed, DType::F16, "gpu-a")?
        );
        changed.prompt.push_str("different");
        assert_ne!(
            key,
            conditioning_key(&files, &changed, DType::F16, "gpu-a")?
        );
        changed = request.clone();
        changed.negative.push_str("different");
        assert_ne!(
            key,
            conditioning_key(&files, &changed, DType::F16, "gpu-a")?
        );
        changed = request.clone();
        changed.guidance = 1.0;
        assert_ne!(
            key,
            conditioning_key(&files, &changed, DType::F16, "gpu-a")?
        );
        assert_ne!(
            key,
            conditioning_key(&files, &request, DType::F32, "gpu-a")?
        );
        assert_ne!(
            key,
            conditioning_key(&files, &request, DType::F16, "gpu-b")?
        );
        files.tokenizer_identity.push('2');
        assert_ne!(
            key,
            conditioning_key(&files, &request, DType::F16, "gpu-a")?
        );
        files.tokenizer_identity.pop();
        files.signature.push('2');
        assert_ne!(
            key,
            conditioning_key(&files, &request, DType::F16, "gpu-a")?
        );
        files.signature.pop();
        let replacement = directory.path().join("replacement");
        std::fs::write(&replacement, b"encoder-v2-different")?;
        files.g = Some(replacement.clone());
        assert_ne!(
            key,
            conditioning_key(&files, &request, DType::F16, "gpu-a")?
        );
        files.g = Some(files.l.clone());
        files.l = replacement;
        assert_ne!(
            key,
            conditioning_key(&files, &request, DType::F16, "gpu-a")?
        );
        Ok(())
    }
    #[test]
    fn checkpoint_vae_cache_checks_all_variants_without_cross_repo_fallback() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let base = dir.path().join("models--local--one");
        std::fs::create_dir_all(base.join("refs"))?;
        std::fs::create_dir_all(base.join("snapshots/abc/vae"))?;
        std::fs::write(base.join("refs/main"), "abc\n")?;
        let fp16 = base.join("snapshots/abc/vae/half");
        std::fs::write(&fp16, b"checkpoint")?;
        assert_eq!(
            cached_vae(dir.path(), "local/one", &["vae/full", "vae/half"]),
            Some(fp16)
        );
        assert!(cached_vae(dir.path(), "local/two", &["vae/half"]).is_none());
        std::fs::write(base.join("refs/main"), "../abc")?;
        assert!(cached_vae(dir.path(), "local/one", &["vae/half"]).is_none());
        Ok(())
    }
    #[test]
    fn sdxl_vae_uses_fp32_while_denoiser_and_other_variants_keep_their_dtype() {
        assert_eq!(vae_dtype(SdVariant::Sdxl, DType::F16), DType::F32);
        assert_eq!(vae_dtype(SdVariant::Sdxl, DType::F32), DType::F32);
        assert_eq!(vae_dtype(SdVariant::Sd15, DType::F16), DType::F16);
    }
}

#[cfg(all(test, feature = "plakat-cuda"))]
#[path = "vae_probe.rs"]
mod vae_probe;
