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
    let repo = std::env::var("VAE_PROBE_MODEL")
        .map_err(|_| anyhow!("Set VAE_PROBE_MODEL to a prepared local checkpoint"))?;
    let request = Request {
        model: repo.clone(),
        ..Default::default()
    };
    let path = tokio::runtime::Runtime::new()?.block_on(resolve_vae(&request, &repo))?;
    let expected = PathBuf::from(
        std::env::var("VAE_PROBE_EXPECTED").map_err(|_| anyhow!("Set VAE_PROBE_EXPECTED"))?,
    );
    if std::fs::canonicalize(&path)? != std::fs::canonicalize(expected)? {
        bail!("Resolved VAE differs from the selected checkpoint");
    }
    let runtime = mufoed_model_runtime::Runtime::global();
    let mut task = mufoed_model_runtime::Task::new("image", "fixed VAE diagnostic");
    task.memory = Memory {
        ram: Some(4 << 30),
        vram: Some(4 << 30),
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
    let source: Vec<f32> = (0..3 * 512 * 512)
        .map(|i| (i as f32 * 0.003).sin() * 0.5)
        .collect();
    let source = Tensor::from_vec(source, (1, 3, 512, 512), &device)?;
    device.set_seed(42)?;
    let began = Instant::now();
    let watch = profiling::watch_memory("vae.encode_memory", &device);
    let encoded = model.encode(&source)?.sample()?;
    device.synchronize()?;
    let encode_seconds = began.elapsed().as_secs_f64();
    drop(watch);
    if !encoded.sum_all()?.to_scalar::<f32>()?.is_finite() {
        bail!("Non-finite checkpoint VAE encode");
    }
    drop(encoded);
    let input: Vec<f32> = (0..4 * 64 * 64)
        .map(|i| ((i as f32) * 0.017).sin() * 0.5)
        .collect();
    let input = Tensor::from_vec(input, (1, 4, 64, 64), &device)?;
    let mut times = Vec::new();
    for index in 0..2 {
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
            save_image(&image, root.join(format!("artifacts/vae-{tag}.png")))?;
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
            "seconds":times,"cudnn":cfg!(feature="plakat-cudnn"),"shape":[1,4,64,64],"weights":fingerprint(&path)?,"resolved_path":path,"model":repo,"encode_seconds":encode_seconds
        }))?,
    )?;
    Ok(())
}

fn save_image(image: &Tensor, path: PathBuf) -> Result<()> {
    let bytes = (((image / 2.0)? + 0.5)?.clamp(0f32, 1f32)? * 255.0)?
        .to_dtype(DType::U8)?
        .i(0)?
        .permute((1, 2, 0))?
        .flatten_all()?
        .to_vec1::<u8>()?;
    Raster::new(
        512,
        512,
        bytes
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
    )?
    .save_png(path)?;
    Ok(())
}
