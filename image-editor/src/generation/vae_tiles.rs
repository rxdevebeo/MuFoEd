//! Bounded VAE workspace for large rasters. Overlaps blend on the host, never a full GPU canvas.
//! Tiling changes VAE context/normalization slightly; it is not bit-identical to full-frame VAE.
use anyhow::{bail, Result};
use candle_core::{DType, Device, Tensor};
use candle_transformers::models::stable_diffusion::vae::AutoEncoderKL;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) fn needed(size: [u32; 2]) -> bool {
    u64::from(size[0]) * u64::from(size[1]) > 1024 * 1024
}
pub(super) fn encode(
    model: &AutoEncoderKL,
    pixels: &Tensor,
    cancel: &AtomicBool,
) -> Result<Tensor> {
    let (_, _, h, w) = pixels.dims4()?;
    if h * w <= 1024 * 1024 {
        return Ok(model.encode(pixels)?.sample()?);
    }
    map(pixels, 512, 128, 1, 8, cancel, |tile| {
        Ok(model.encode(tile)?.sample()?)
    })
}
pub(super) fn decode(
    model: &AutoEncoderKL,
    latents: &Tensor,
    cancel: &AtomicBool,
) -> Result<Tensor> {
    let (_, _, h, w) = latents.dims4()?;
    if h * w <= 128 * 128 {
        return Ok(model.decode(latents)?);
    }
    map(
        latents,
        64,
        16,
        8,
        1,
        cancel,
        |tile| Ok(model.decode(tile)?),
    )
}
fn starts(length: usize, tile: usize, overlap: usize) -> Vec<usize> {
    if length <= tile {
        return vec![0];
    }
    let mut values = vec![0];
    while values.last().copied().unwrap() + tile < length {
        let next = (values.last().copied().unwrap() + tile - overlap).min(length - tile);
        values.push(next);
    }
    values
}
#[allow(clippy::cast_precision_loss)] // Positions are bounded by the 512px VAE tile.
fn feather(position: usize, length: usize, before: usize, after: usize) -> f32 {
    let left = if before == 0 {
        1.0
    } else {
        ((position + 1) as f32 / (before + 1) as f32).min(1.0)
    };
    let right = if after == 0 {
        1.0
    } else {
        ((length - position) as f32 / (after + 1) as f32).min(1.0)
    };
    left.min(right)
}
#[allow(clippy::too_many_arguments)] // Explicit integer scale keeps encode/decode coordinates identical.
fn map(
    input: &Tensor,
    tile: usize,
    overlap: usize,
    numerator: usize,
    denominator: usize,
    cancel: &AtomicBool,
    mut operation: impl FnMut(&Tensor) -> Result<Tensor>,
) -> Result<Tensor> {
    let (batch, _, height, width) = input.dims4()?;
    if batch != 1 || height % denominator != 0 || width % denominator != 0 {
        bail!("Tiled VAE requires one image and scale-aligned dimensions");
    }
    let (out_h, out_w) = (
        height * numerator / denominator,
        width * numerator / denominator,
    );
    let rows = starts(height, tile, overlap);
    let columns = starts(width, tile, overlap);
    let mut canvas = Vec::new();
    let mut weights = vec![0f32; out_h * out_w];
    let mut channels = 0;
    let mut dtype = input.dtype();
    for (row, &y) in rows.iter().enumerate() {
        for (column, &x) in columns.iter().enumerate() {
            if cancel.load(Ordering::Acquire) {
                bail!("VAE cancelled between tiles");
            }
            let (h, w) = (tile.min(height - y), tile.min(width - x));
            let forward_timing =
                crate::profiling::gpu("vae.tile_forward_and_transfer", input.device())?;
            let patch = input.narrow(2, y, h)?.narrow(3, x, w)?.contiguous()?;
            let output = operation(&patch)?;
            let (b, c, oh, ow) = output.dims4()?;
            if b != 1 || oh != h * numerator / denominator || ow != w * numerator / denominator {
                bail!("Tiled VAE returned unexpected dimensions");
            }
            if canvas.is_empty() {
                channels = c;
                dtype = output.dtype();
                canvas.resize(channels * out_h * out_w, 0f32);
            } else if c != channels {
                bail!("Tiled VAE returned inconsistent channels");
            }
            let data = output
                .to_dtype(DType::F32)?
                .to_device(&Device::Cpu)?
                .flatten_all()?
                .to_vec1::<f32>()?;
            drop(forward_timing);
            let _blend = crate::profiling::scope("vae.tile_cpu_blend");
            if data.iter().any(|v| !v.is_finite()) {
                bail!("VAE tile produced non-finite values");
            }
            let (oy, ox) = (y * numerator / denominator, x * numerator / denominator);
            let before_y = if row == 0 {
                0
            } else {
                (rows[row - 1] + tile - y) * numerator / denominator
            };
            let after_y = rows
                .get(row + 1)
                .map_or(0, |next| (y + h - next) * numerator / denominator);
            let before_x = if column == 0 {
                0
            } else {
                (columns[column - 1] + tile - x) * numerator / denominator
            };
            let after_x = columns
                .get(column + 1)
                .map_or(0, |next| (x + w - next) * numerator / denominator);
            for py in 0..oh {
                for px in 0..ow {
                    let weight =
                        feather(py, oh, before_y, after_y) * feather(px, ow, before_x, after_x);
                    let destination = (oy + py) * out_w + ox + px;
                    weights[destination] += weight;
                    for channel in 0..channels {
                        canvas[channel * out_h * out_w + destination] +=
                            data[channel * oh * ow + py * ow + px] * weight;
                    }
                }
            }
        }
    }
    for channel in canvas.chunks_exact_mut(out_h * out_w) {
        for (value, weight) in channel.iter_mut().zip(&weights) {
            if *weight <= 0.0 {
                bail!("Tiled VAE left uncovered pixels");
            }
            *value /= weight;
        }
    }
    Ok(
        Tensor::from_vec(canvas, (1, channels, out_h, out_w), &Device::Cpu)?
            .to_dtype(dtype)?
            .to_device(input.device())?,
    )
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss)]
mod tests {
    use super::*;
    #[test]
    fn tiled_identity_preserves_rectangular_ramps_and_overlap_boundaries() -> Result<()> {
        for (h, w) in [(16, 24), (72, 128), (128, 72), (104, 104)] {
            let values: Vec<f32> = (0..3 * h * w).map(|i| i as f32 / 100.0).collect();
            let input = Tensor::from_vec(values.clone(), (1, 3, h, w), &Device::Cpu)?;
            let output = map(&input, 64, 16, 1, 1, &AtomicBool::new(false), |t| {
                Ok(t.clone())
            })?;
            for (a, b) in output.flatten_all()?.to_vec1::<f32>()?.iter().zip(values) {
                assert!((a - b).abs() < 0.0002);
            }
        }
        Ok(())
    }
    #[test]
    fn tile_scaling_keeps_coordinates_channels_and_dimensions() -> Result<()> {
        let input = Tensor::ones((1, 4, 72, 104), DType::F32, &Device::Cpu)?;
        let decoded = map(&input, 64, 16, 8, 1, &AtomicBool::new(false), |t| {
            let (_, _, h, w) = t.dims4()?;
            Ok(Tensor::ones(
                (1, 3, h * 8, w * 8),
                DType::F32,
                &Device::Cpu,
            )?)
        })?;
        assert_eq!(decoded.dims(), &[1, 3, 576, 832]);
        assert!(decoded
            .flatten_all()?
            .to_vec1::<f32>()?
            .iter()
            .all(|v| (*v - 1.0).abs() < 1e-6));
        let encoded = map(&decoded, 512, 128, 1, 8, &AtomicBool::new(false), |t| {
            let (_, _, h, w) = t.dims4()?;
            Ok(Tensor::ones(
                (1, 4, h / 8, w / 8),
                DType::F32,
                &Device::Cpu,
            )?)
        })?;
        assert_eq!(encoded.dims(), input.dims());
        assert!(encoded
            .flatten_all()?
            .to_vec1::<f32>()?
            .iter()
            .all(|v| (*v - 1.0).abs() < 1e-6));
        Ok(())
    }
    #[test]
    fn invalid_tiles_and_cancel_between_tiles_produce_no_partial_output() -> Result<()> {
        let input = Tensor::ones((1, 3, 72, 104), DType::F32, &Device::Cpu)?;
        let cancel = AtomicBool::new(false);
        let mut count = 0;
        assert!(map(&input, 64, 16, 1, 1, &cancel, |t| {
            count += 1;
            cancel.store(true, Ordering::Release);
            Ok(t.clone())
        })
        .is_err());
        assert_eq!(count, 1);
        assert!(map(&input, 64, 16, 1, 1, &AtomicBool::new(false), |t| {
            Ok((t * f64::NAN)?)
        })
        .is_err());
        assert!(map(&input, 64, 16, 1, 1, &AtomicBool::new(false), |_| {
            Ok(Tensor::ones((1, 3, 1, 1), DType::F32, &Device::Cpu)?)
        })
        .is_err());
        Ok(())
    }
    #[test]
    fn cancelled_tiles_do_not_start_inference() -> Result<()> {
        let input = Tensor::zeros((1, 3, 72, 72), DType::F32, &Device::Cpu)?;
        assert!(
            map(&input, 64, 16, 1, 1, &AtomicBool::new(true), |_| panic!(
                "cancelled"
            ))
            .is_err()
        );
        Ok(())
    }
}
