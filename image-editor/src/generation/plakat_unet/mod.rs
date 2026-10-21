//! Minimal attributed Plakat `UNet` adapter, see UPSTREAM.json/LICENSE.
//! Architecture/weights/conditioning unchanged. Fixes upstream head slicing shape/remainder.
//! Public Plakat `LoRA`, scheduler, CLIP, VAE and other pipelines remain dependencies.
mod attention;
mod blocks;
mod unet;
use candle_core::{Result, Tensor};
use candle_nn::VarBuilder;
use plakat::pipelines::sdxl_unet::SdxlAddEmbedConfig;
pub(super) struct Model(unet::UNet2DConditionModel);
impl Model {
    pub(super) fn new(
        vb: VarBuilder<'_>,
        channels: usize,
        cfg: candle_transformers::models::stable_diffusion::unet_2d::UNet2DConditionModelConfig,
        added: Option<SdxlAddEmbedConfig>,
    ) -> Result<Self> {
        Ok(Self(unet::UNet2DConditionModel::new(
            vb, channels, 4, false, cfg, added,
        )?))
    }
    pub(super) fn forward(
        &self,
        input: &Tensor,
        time: f64,
        hidden: &Tensor,
        pooled: Option<&Tensor>,
        ids: Option<&Tensor>,
    ) -> Result<Tensor> {
        match (pooled, ids) {
            (Some(pooled), Some(ids)) => self.0.forward_sdxl(input, time, hidden, pooled, ids),
            (None, None) => self.0.forward(input, time, hidden),
            _ => candle_core::bail!("Incomplete SDXL conditioning"),
        }
    }
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss)] // Tiny diagnostic tensors only.
mod parity_tests {
    use super::*;
    use candle_core::{DType, Device};
    use candle_nn::VarMap;
    use std::collections::HashMap;
    fn signal(shape: &[usize], scale: f32) -> Result<Tensor> {
        let data: Vec<f32> = (0..shape.iter().product())
            .map(|i| (i as f32 * 0.17).sin() * scale)
            .collect();
        Tensor::from_vec(data, shape, &Device::Cpu)
    }
    #[test]
    fn sdxl_unet_bridge_preserves_upstream_weights_cfg_and_microconditioning() -> Result<()> {
        let device = Device::Cpu;
        let mut config = plakat::pipelines::controlnet::sdxl_unet_config();
        for (block, channels) in config.blocks.iter_mut().zip([32, 64, 128]) {
            block.out_channels = channels;
            block.attention_head_dim = channels / 32;
            block.use_cross_attn = block.use_cross_attn.map(|_| 1);
        }
        config.cross_attention_dim = 16;
        config.layers_per_block = 1;
        let map = VarMap::new();
        let reference = plakat::pipelines::sd_train::unet::UNet2DConditionModel::new(
            VarBuilder::from_varmap(&map, DType::F32, &device),
            4,
            4,
            false,
            config.clone(),
            Some(SdxlAddEmbedConfig::base()),
        )?;
        let tensors: HashMap<_, _> = map
            .data()
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_tensor().clone()))
            .collect();
        config.sliced_attention_size = Some(2);
        let candidate = Model::new(
            VarBuilder::from_tensors(tensors, DType::F32, &device),
            4,
            config,
            Some(SdxlAddEmbedConfig::base()),
        )?;
        for batch in [1, 2] {
            let input = signal(&[batch, 4, 16, 16], 0.2)?;
            let hidden = signal(&[batch, 7, 16], 0.1)?;
            let pooled = signal(&[batch, 1280], 0.1)?;
            let ids = Tensor::from_vec(
                [1024f32, 1024.0, 0.0, 0.0, 1024.0, 1024.0].repeat(batch),
                (batch, 6),
                &device,
            )?;
            let expected = reference.forward_sdxl(&input, 500.0, &hidden, &pooled, &ids)?;
            let actual = candidate.forward(&input, 500.0, &hidden, Some(&pooled), Some(&ids))?;
            let delta = (&actual - &expected)?
                .abs()?
                .max_all()?
                .to_scalar::<f32>()?;
            assert!(delta < 2e-4, "UNet delta {delta}");
            let altered = candidate.forward(
                &input,
                500.0,
                &hidden,
                Some(&pooled.zeros_like()?),
                Some(&ids),
            )?;
            assert!((&altered - &actual)?.abs()?.max_all()?.to_scalar::<f32>()? > 1e-6);
        }
        Ok(())
    }
}
