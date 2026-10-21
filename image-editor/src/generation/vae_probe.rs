//! Explicit fixed-latent CUDA diagnostic; never runs in the ordinary test suite.
use super::*;
use std::time::{Duration, Instant};
#[test]
#[ignore = "Explicit local CUDA VAE comparison, owns a runtime admission"]
fn fixed_latent_vae_probe() {
    let (sender, receiver) = std::sync::mpsc::channel();
    super::super::worker::dispatch(Box::new(move || {
        let result = probe().map_err(|e| format!("{e:#}"));
        let _ = sender.send(result);
    }))
    .unwrap();
    receiver
        .recv_timeout(Duration::from_secs(600))
        .unwrap()
        .unwrap();
}
#[allow(clippy::cast_precision_loss)] // Diagnostic latent indices are < 65536.
fn probe() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tag = std::env::var("VAE_PROBE_TAG").unwrap_or_else(|_| "probe".into());
    if !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        bail!("Invalid VAE_PROBE_TAG");
    }
    let path = root.join(".runtime/hub/models--madebyollin--sdxl-vae-fp16-fix/snapshots/195eed7a7b1d493afdd8d66017a117ab5300b150/diffusion_pytorch_model.safetensors");
    let runtime = mufoed_model_runtime::Runtime::global();
    let mut task = mufoed_model_runtime::Task::new("image", "fixed VAE diagnostic");
    task.memory = Memory {
        ram: Some(4 << 30),
        vram: Some(10 << 30),
    };
    let lease = runtime
        .enqueue(task)?
        .wait(&AtomicBool::new(false), Duration::from_secs(120))?;
    let device = execution_device(true, &lease.device_identity("auto")?)?;
    let weights = Weights::load(&path, DType::F32)?;
    let model = AutoEncoderKL::new(
        weights.builder(&device, DType::F32)?,
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
    let input: Vec<f32> = (0..4 * 128 * 128)
        .map(|i| ((i as f32) * 0.017).sin() * 0.5)
        .collect();
    let input = Tensor::from_vec(input, (1, 4, 128, 128), &device)?;
    let mut times = Vec::new();
    for index in 0..6 {
        device.synchronize()?;
        let watch = profiling::watch_memory("vae.fixed_memory", &device);
        let began = Instant::now();
        let image = model.decode(&input)?;
        device.synchronize()?;
        let seconds = began.elapsed().as_secs_f64();
        drop(watch);
        let image = image.to_device(&Device::Cpu)?;
        if !image.sum_all()?.to_scalar::<f32>()?.is_finite() {
            bail!("Non-finite fixed VAE output");
        }
        if index == 0 {
            candle_core::safetensors::save(
                &HashMap::from([("image".to_owned(), image)]),
                root.join(format!("artifacts/vae-{tag}.safetensors")),
            )?;
        }
        times.push(seconds);
        println!("VAE {tag} {index}: {seconds:.3}s");
    }
    std::fs::write(
        root.join(format!("artifacts/vae-{tag}.json")),
        serde_json::to_vec_pretty(&serde_json::json!({
            "seconds":times,"cudnn":cfg!(feature="plakat-cudnn"),"shape":[1,4,128,128],"weights":fingerprint(&path)?
        }))?,
    )?;
    Ok(())
}
