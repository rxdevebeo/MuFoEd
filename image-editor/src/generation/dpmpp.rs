//! DPM-Solver++ 2M midpoint, Karras rho=7; epsilon prediction in VP coordinates.
//! Numerical reference: Diffusers `DPMSolverMultistepScheduler`, `use_karras_sigmas=True`,
//! `algorithm_type=dpmsolver++`, `solver_order=2`, `solver_type=midpoint`, `final_sigmas_type=zero`.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
use anyhow::{bail, Result};
use candle_core::{DType, Tensor};
use candle_transformers::models::stable_diffusion::schedulers::Scheduler;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(default)]
pub(super) struct Config {
    beta_start: f64,
    beta_end: f64,
    beta_schedule: String,
    num_train_timesteps: usize,
    prediction_type: String,
    rescale_betas_zero_snr: bool,
    trained_betas: Option<Vec<f64>>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            beta_start: 0.00085,
            beta_end: 0.012,
            beta_schedule: "scaled_linear".into(),
            num_train_timesteps: 1000,
            prediction_type: "epsilon".into(),
            rescale_betas_zero_snr: false,
            trained_betas: None,
        }
    }
}
pub(super) struct Dpmpp {
    times: Vec<usize>,
    sigmas: Vec<f64>,
    index: usize,
    previous: Option<(Tensor, f64)>,
}
impl Dpmpp {
    pub(super) fn new(config: &Config, steps: usize, begin: usize) -> Result<Self> {
        if !(1..=150).contains(&steps)
            || begin >= steps
            || !(2..=10000).contains(&config.num_train_timesteps)
            || config.prediction_type != "epsilon"
            || config.rescale_betas_zero_snr
        {
            bail!("DPM++ 2M supports epsilon models without zero-SNR rescaling; invalid schedule or unsupported model config");
        }
        let train = config.num_train_timesteps;
        let mut training = Vec::with_capacity(train);
        let mut product = 1.0;
        if !config.beta_start.is_finite()
            || !config.beta_end.is_finite()
            || config.beta_start <= 0.0
            || config.beta_end >= 1.0
            || config.beta_start > config.beta_end
        {
            bail!("Invalid training beta range");
        }
        if config
            .trained_betas
            .as_ref()
            .is_some_and(|b| b.len() != train)
        {
            bail!("Trained beta count differs from training timesteps");
        }
        for i in 0..train {
            let ratio = i as f64 / (train - 1) as f64;
            let beta = if let Some(betas) = &config.trained_betas {
                betas[i]
            } else {
                match config.beta_schedule.as_str() {
                    "linear" => config.beta_start + (config.beta_end - config.beta_start) * ratio,
                    "scaled_linear" => {
                        let root = (config.beta_start.sqrt()
                            + (config.beta_end.sqrt() - config.beta_start.sqrt()) * ratio)
                            as f32;
                        f64::from(root * root)
                    }
                    _ => bail!(
                        "Unsupported DPM++ training beta schedule: {}",
                        config.beta_schedule
                    ),
                }
            };
            if !beta.is_finite() || beta <= 0.0 || beta >= 1.0 {
                bail!("Invalid training beta");
            }
            // Match the f32 training schedule used by PyTorch/Diffusers.
            product *= f64::from(1.0f32 - beta as f32);
            let alpha = product as f32;
            let sigma = f64::from(((1.0 - alpha) / alpha).sqrt());
            if !sigma.is_finite() || sigma <= 0.0 || training.last().is_some_and(|v| *v >= sigma) {
                bail!("Training schedule produces invalid or non-increasing sigmas");
            }
            training.push(sigma);
        }
        let min = training[0];
        let max = training[train - 1];
        let ramp = |i: usize| {
            if steps == 1 {
                0.0
            } else {
                i as f64 / (steps - 1) as f64
            }
        };
        let karras: Vec<f64> = (0..steps)
            .map(|i| {
                (max.powf(1.0 / 7.0) + ramp(i) * (min.powf(1.0 / 7.0) - max.powf(1.0 / 7.0)))
                    .powi(7)
            })
            .collect();
        let logs: Vec<f64> = training
            .iter()
            .map(|v| f64::from((*v as f32).ln()))
            .collect();
        let times = karras
            .iter()
            .map(|sigma| sigma_to_time(*sigma, &logs))
            .collect();
        let mut sigmas: Vec<f64> = karras.iter().map(|s| f64::from(*s as f32)).collect();
        sigmas.push(0.0);
        Ok(Self {
            times,
            sigmas,
            index: begin,
            previous: None,
        })
    }
    fn position(&self, time: usize) -> candle_core::Result<usize> {
        if self.times.get(self.index) == Some(&time) {
            return Ok(self.index);
        }
        self.times
            .iter()
            .position(|&t| t == time)
            .ok_or_else(|| candle_core::Error::Msg("Timestep is outside the DPM++ schedule".into()))
    }
}
fn sigma_to_time(sigma: f64, logs: &[f64]) -> usize {
    let log = sigma.ln();
    let low = logs
        .partition_point(|&v| v <= log)
        .saturating_sub(1)
        .min(logs.len() - 2);
    let fraction = ((log - logs[low]) / (logs[low + 1] - logs[low])).clamp(0.0, 1.0);
    (low as f64 + fraction).round_ties_even() as usize
}
fn alpha_sigma(sigma: f64) -> (f64, f64) {
    let alpha = 1.0 / (1.0 + sigma * sigma).sqrt();
    (alpha, sigma * alpha)
}
impl Scheduler for Dpmpp {
    fn timesteps(&self) -> &[usize] {
        &self.times
    }
    fn init_noise_sigma(&self) -> f64 {
        1.0
    }
    fn scale_model_input(&self, sample: Tensor, _time: usize) -> candle_core::Result<Tensor> {
        Ok(sample)
    }
    fn add_noise(
        &self,
        original: &Tensor,
        noise: Tensor,
        time: usize,
    ) -> candle_core::Result<Tensor> {
        let (alpha, sigma) = alpha_sigma(self.sigmas[self.position(time)?]);
        let dtype = original.dtype();
        ((original.to_dtype(DType::F32)? * alpha)? + (noise.to_dtype(DType::F32)? * sigma)?)?
            .to_dtype(dtype)
    }
    fn step(
        &mut self,
        output: &Tensor,
        time: usize,
        sample: &Tensor,
    ) -> candle_core::Result<Tensor> {
        if self.times.get(self.index) != Some(&time) {
            candle_core::bail!("DPM++ steps must follow the selected schedule in order");
        }
        let current = self.sigmas[self.index];
        let next = self.sigmas[self.index + 1];
        let (alpha_s, sigma_s) = alpha_sigma(current);
        let sample_f32 = sample.to_dtype(DType::F32)?;
        let x0 = ((sample_f32.clone() - (output.to_dtype(DType::F32)? * sigma_s)?)? / alpha_s)?;
        let value = if next <= f64::EPSILON {
            x0.clone()
        } else {
            let (alpha_t, sigma_t) = alpha_sigma(next);
            let h = (current / next).ln();
            let coefficient = -alpha_t * (-h).exp_m1();
            let mut value = ((sample_f32 * (sigma_t / sigma_s))? + (&x0 * coefficient)?)?;
            if let Some((previous, previous_sigma)) = &self.previous {
                let h0 = (previous_sigma / current).ln();
                value = (value + ((&x0 - previous)? * (0.5 * coefficient * h / h0))?)?;
            }
            value
        };
        self.previous = Some((x0, current));
        self.index += 1;
        value.to_dtype(sample.dtype())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use candle_core::Device;
    #[test]
    fn matches_diffusers_steps_sigmas_noising_and_multistep_updates() {
        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/dpmpp_diffusers.json"))
                .unwrap();
        for case in fixtures["cases"].as_array().unwrap() {
            let steps = case["steps"].as_u64().unwrap() as usize;
            let begin = case["begin"].as_u64().unwrap() as usize;
            let mut solver = Dpmpp::new(&Config::default(), steps, begin).unwrap();
            let times: Vec<usize> = serde_json::from_value(case["timesteps"].clone()).unwrap();
            assert_eq!(solver.timesteps(), times);
            let sigmas: Vec<f64> = serde_json::from_value(case["sigmas"].clone()).unwrap();
            for (a, b) in solver.sigmas.iter().zip(sigmas) {
                assert!((a - b).abs() < 2e-5, "sigma {a} vs {b}");
            }
            let mut sample =
                Tensor::from_vec(vec![0.2f32, -0.4, 0.7, 1.0], (1, 4), &Device::Cpu).unwrap();
            if begin > 0 {
                let noise =
                    Tensor::from_vec(vec![0.3f32, 0.1, -0.2, 0.5], (1, 4), &Device::Cpu).unwrap();
                sample = solver.add_noise(&sample, noise, times[begin]).unwrap();
                compare(&sample, &case["noised"]);
            }
            for (i, &time) in times.iter().enumerate().skip(begin) {
                let output =
                    Tensor::from_vec(vec![0.01f32 * (i + 1) as f32; 4], (1, 4), &Device::Cpu)
                        .unwrap();
                sample = solver.step(&output, time, &sample).unwrap();
                compare(&sample, &case["outputs"][i - begin]);
            }
        }
    }
    fn compare(tensor: &Tensor, expected: &serde_json::Value) {
        let actual = tensor.flatten_all().unwrap().to_vec1::<f32>().unwrap();
        let expected: Vec<f32> = serde_json::from_value(expected.clone()).unwrap();
        for (a, b) in actual.iter().zip(expected) {
            assert!((a - b).abs() < 2e-4, "sample {a} vs {b}");
        }
    }
    #[test]
    fn unsupported_prediction_and_out_of_order_steps_fail() {
        let config = Config {
            prediction_type: "v_prediction".into(),
            ..Default::default()
        };
        assert!(Dpmpp::new(&config, 20, 0).is_err());
        let mut solver = Dpmpp::new(&Config::default(), 4, 0).unwrap();
        let sample = Tensor::zeros((1, 4), DType::F32, &Device::Cpu).unwrap();
        assert!(solver.step(&sample, 0, &sample).is_err());
    }
}
