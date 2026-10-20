//! Staged Plakat SD adapter, pinned to `PLAKAT_REFERENCE`. No `StrictLib` or Plakat fork.
//! Uses Plakat models, `LoRA` merging, prompt weighting, masks, scheduler and preprocessing.
//! CPU tensors are loaded once per fingerprint; each GPU stage is destroyed and synchronized
//! before the next component is constructed. This trades `PCIe` transfer for a bounded peak.
use super::{Mode, Request};
use crate::Raster;
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
    sdxl_unet::{SdUNet, SdxlAddEmbedConfig},
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
    fn builder(&self, device: &Device, dtype: DType) -> Result<VarBuilder<'static>> {
        let map = self
            .tensors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let tensors = map
            .as_ref()
            .ok_or_else(|| anyhow!("evicted pinned weights"))?
            .iter()
            .map(|(k, v)| Ok((k.clone(), v.to_dtype(dtype)?.to_device(device)?)))
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
    tok_l: Tokenizer,
    tok_g: Option<Tokenizer>,
    variant: SdVariant,
    loras: Vec<ResolvedLora>,
    signature: String,
}
fn resolve_loras(request: &Request, variant: SdVariant) -> Result<(Vec<ResolvedLora>, String)> {
    let mut loras = Vec::new();
    let mut signature = format!("{}:{variant:?}", super::PLAKAT_REFERENCE);
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
impl Files {
    async fn resolve(request: &Request, gpu: bool) -> Result<Self> {
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
        let vae = if let Some(path) = &request.vae {
            path.clone()
        } else if variant.is_xl() && gpu {
            first(
                "madebyollin/sdxl-vae-fp16-fix",
                &[
                    "diffusion_pytorch_model.safetensors",
                    "sdxl_vae.safetensors",
                    "sdxl.vae.safetensors",
                ],
            )
            .await?
        } else {
            first(
                &repo,
                &[
                    "vae/diffusion_pytorch_model.fp16.safetensors",
                    "vae/diffusion_pytorch_model.safetensors",
                ],
            )
            .await?
        };
        let (loras, signature) = resolve_loras(request, variant)?;
        Ok(Self {
            l,
            g,
            unet,
            vae,
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
            "plakat:{:?}:{}:{uuid}:{}:{}",
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
    ) -> Result<Pinned<'lease, Weights>> {
        let key = self.key(path, target.as_ref(), dtype, uuid)?;
        let resource = lease
            .cached(&key, || {
                let merged = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
                let effective = if let Some(target) = target {
                    if self.loras.is_empty() {
                        path
                    } else {
                        plakat::pipelines::lora::merge_loras_into_weights(
                            path,
                            merged.path(),
                            &self.loras,
                            1.0,
                            &Device::Cpu,
                            target,
                        )
                        .map_err(|e| e.to_string())?;
                        merged.path()
                    }
                } else {
                    path
                };
                let tensors = candle_core::safetensors::load(effective, &Device::Cpu)
                    .map_err(|e| e.to_string())?;
                let bytes = tensors
                    .values()
                    .map(|t| (t.elem_count() * t.dtype().size_in_bytes()) as u64)
                    .sum();
                eprintln!(
                    "[resident] cold weight load: {} ({bytes} bytes)",
                    path.display()
                );
                Ok(Weights {
                    tensors: Mutex::new(Some(tensors)),
                    bytes,
                    alive: AtomicBool::new(true),
                })
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
fn tokens(
    tok: &Tokenizer,
    cfg: &stable_diffusion::clip::Config,
    text: &str,
    device: &Device,
    dtype: DType,
) -> Result<(Tensor, Option<Tensor>)> {
    let pad = tok
        .token_to_id(cfg.pad_with.as_deref().unwrap_or("<|endoftext|>"))
        .ok_or_else(|| anyhow!("missing CLIP padding token"))?;
    if plakat::prompt::a1111::has_attention_syntax(text) {
        let config = plakat::prompt::weighted_encoding::WeightedTokenConfig {
            tokenizer: tok,
            max_len: cfg.max_position_embeddings,
            bos_id: tok.token_to_id("<|startoftext|>"),
            eos_id: tok
                .token_to_id("<|endoftext|>")
                .ok_or_else(|| anyhow!("missing EOT"))?,
            pad_id: pad,
        };
        let (ids, weights) = plakat::prompt::weighted_encoding::tokenize_weighted(
            &config,
            &plakat::prompt::a1111::parse(text),
            device,
            dtype,
        )?;
        Ok((ids, Some(weights)))
    } else {
        let mut ids = tok
            .encode(text, true)
            .map_err(|e| anyhow!(e.to_string()))?
            .get_ids()
            .to_vec();
        ids.resize(cfg.max_position_embeddings, pad);
        Ok((Tensor::new(ids.as_slice(), device)?.unsqueeze(0)?, None))
    }
}
fn apply_weights(hidden: Tensor, weights: Option<Tensor>) -> Result<Tensor> {
    Ok(if let Some(weights) = weights {
        hidden.broadcast_mul(&weights.to_dtype(hidden.dtype())?)?
    } else {
        hidden
    })
}
fn vae<'lease>(
    files: &Files,
    lease: &'lease Lease,
    device: &Device,
    dtype: DType,
    uuid: &str,
) -> Result<(AutoEncoderKL, Pinned<'lease, Weights>)> {
    let weights = files.weights(&files.vae, None, lease, dtype, uuid)?;
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
    fn encode_l(&self, texts: &[Vec<String>]) -> Result<Tensor> {
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
        self.lease.stage(Phase::Running, "CLIP-L encode");
        let mut branches = Vec::new();
        for chunks in texts {
            let mut outputs = Vec::new();
            for text in chunks {
                let (ids, weights) = tokens(
                    &self.files.tok_l,
                    &self.cfg.clip,
                    text,
                    &self.device,
                    self.dtype,
                )?;
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
    fn encode_g(&self, texts: &[Vec<String>]) -> Result<(Tensor, Tensor)> {
        self.lease.stage(Phase::Loading, "CLIP-G device placement");
        let weights = self.files.weights(
            self.files.g.as_ref().expect("XL encoder"),
            Some(MergeTarget::TE2_SDXL),
            self.lease,
            self.dtype,
            &self.uuid,
        )?;
        let model = plakat::pipelines::sdxl_clip::SdxlClipGTextTransformer::new(
            weights.builder(&self.device, self.dtype)?,
            &plakat::pipelines::vendored_clip::Config::sdxl2(),
            1280,
        )?;
        self.lease.stage(Phase::Running, "CLIP-G encode");
        let mut branches = Vec::new();
        let mut pools = Vec::new();
        for chunks in texts {
            let mut outputs = Vec::new();
            for (index, text) in chunks.iter().enumerate() {
                let (ids, weights) = tokens(
                    self.files.tok_g.as_ref().expect("XL tokenizer"),
                    self.cfg.clip2.as_ref().expect("XL config"),
                    text,
                    &self.device,
                    self.dtype,
                )?;
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
        let mut texts = vec![];
        if request.guidance > 1.0 {
            texts.push(plakat::prompt::break_chunks::split(&request.negative));
        }
        texts.push(plakat::prompt::break_chunks::split(&request.prompt));
        let count = texts.iter().map(Vec::len).max().unwrap_or(1);
        for chunks in &mut texts {
            chunks.resize(count, String::new());
        }
        let hidden_l = self.encode_l(&texts)?;
        self.lease.stage(
            Phase::Offloading,
            "CLIP-L device model released; CPU cache retained",
        );
        self.device.synchronize()?;
        check(cancel)?;
        let value = if self.files.variant.is_xl() {
            let (hidden_g, pooled) = self.encode_g(&texts)?;
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
    fn init(&self, request: &Request, input: &Path) -> Result<Init> {
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
        self.lease.stage(Phase::Loading, "VAE encode");
        let (model, _weights) = vae(
            &self.files,
            self.lease,
            &self.device,
            self.dtype,
            &self.uuid,
        )?;
        let pixels = plakat::imaging::preprocess::sd_image_tensor(
            &input.join("input.png"),
            width,
            height,
            &self.device,
            self.dtype,
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
                    .to_dtype(self.dtype)?;
            Some(
                (model.encode(&pixels.broadcast_mul(&keep)?)?.sample()?
                    * self.files.variant.vae_scale())?,
            )
        } else {
            None
        };
        let _ = self.device.set_seed(plakat::pipelines::seeds::prepare_seed(
            request.seed,
            &self.device,
        ));
        let base = Some((model.encode(&pixels)?.sample()? * self.files.variant.vae_scale())?);
        Ok(Init {
            base,
            masked,
            mask,
            inverse,
        })
    }
    fn unet(&self) -> Result<SdUNet> {
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
        let model = SdUNet::SdOwn(
            plakat::pipelines::sd_train::unet::UNet2DConditionModel::new(
                self.unet_weights.builder(&self.device, self.dtype)?,
                self.channels,
                4,
                false,
                cfg,
                self.files.variant.is_xl().then(SdxlAddEmbedConfig::base),
            )?,
        );
        Ok(model)
    }
    fn denoise(
        &self,
        request: &Request,
        conditioning: &Conditioning,
        init: &Init,
        cancel: &AtomicBool,
    ) -> Result<Tensor> {
        let do_cfg = request.guidance > 1.0;
        let mut scheduler = plakat::pipelines::scheduler::build(
            plakat::pipelines::scheduler::SchedulerKind::default(),
            &self.cfg,
            request.steps as usize,
        )?;
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
        self.lease.stage(Phase::Running, "UNet denoise");
        for &time in &times[start..] {
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
            let prediction = model.forward(
                &sample,
                time as f64,
                &conditioning.hidden,
                conditioning.pooled.as_ref(),
                time_ids.as_ref(),
            )?;
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
    fn decode(&self, latents: &Tensor) -> Result<Raster> {
        self.lease.stage(Phase::Loading, "VAE decode");
        let (model, _weights) = vae(
            &self.files,
            self.lease,
            &self.device,
            self.dtype,
            &self.uuid,
        )?;
        let image = model.decode(&(latents / self.files.variant.vae_scale())?)?;
        if !image
            .to_dtype(DType::F32)?
            .sum_all()?
            .to_scalar::<f32>()?
            .is_finite()
        {
            bail!("VAE produced non-finite values; select an fp16-safe VAE or restore the model default");
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
    let files = Files::resolve(request, gpu).await?;
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
        files.key(&files.vae, None, dtype, &uuid)?,
    ];
    if let Some(g) = &files.g {
        keys.push(files.key(g, Some(&MergeTarget::TE2_SDXL), dtype, &uuid)?);
    }
    lease.prepare_reusing(
        super::native::estimate(request, [width, height]),
        &uuid,
        &keys,
    )?;
    check(cancel)?;
    let device = plakat::api::device(if gpu { "auto" } else { "cpu" })?;
    if gpu && matches!(device, Device::Cpu) {
        bail!("CUDA initialization failed; CPU execution must be selected explicitly");
    }
    let _sync = Synchronize(device.clone());
    let weights = files.weights(&files.unet, Some(MergeTarget::UNET), lease, dtype, &uuid)?;
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
    let init = staged.init(request, input)?;
    if init.base.is_some() && !matches!(staged.device, Device::Cpu) {
        lease.stage(Phase::Offloading, "VAE encoder GPU released");
    }
    staged.device.synchronize()?;
    check(cancel)?;
    let latents = staged.denoise(request, &conditioning, &init, cancel)?;
    staged.device.synchronize()?;
    check(cancel)?;
    lease.stage(
        Phase::Offloading,
        "UNet device model released; CPU cache retained",
    );
    let raster = staged.decode(&latents)?;
    staged.device.synchronize()?;
    Ok(raster)
}
