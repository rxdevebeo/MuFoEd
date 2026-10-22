//! Explicit CUDA Hires Fix acceptance using already prepared local models.
use image_editor::{
    generation::{HiresFix, PipelineJob, Postprocess, Request, Sampler},
    Document,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cache = root.join(".runtime/hub");
    std::env::set_var("PLAKAT_CACHE_DIR", &cache);
    std::env::set_var("PLAKAT_NO_SDPA", "1");
    let runtime = mufoed_model_runtime::Runtime::global();
    let base: u32 = std::env::var("HIRES_SMOKE_BASE")
        .unwrap_or_else(|_| "512".into())
        .parse()?;
    let target = base * 3 / 2;
    let doc = Document::new(base, base)?;
    let before = doc.clone();
    let request = Request {
        model: "local/image-studio-sdxl".into(),
        family: Some(image_editor::assets::Family::Sdxl),
        cache_dir: Some(cache),
        output_size: Some([base, base]),
        prompt: "a blue ceramic vase on a wooden table, studio still life, soft light".into(),
        sampler: Sampler::Dpmpp2mKarras,
        steps: 10,
        seed: 42,
        postprocess: Some(Postprocess {
            hires: Some(HiresFix {
                target_size: [target, target],
                guidance: 6.0,
            }),
            denoise: 0.3,
            steps: 10,
            ultrasharp: root.join(".runtime/Upscale/ultrasharp/4x-UltraSharp.safetensors"),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut job = PipelineJob::start_with_runtime(true, "", request.clone(), &doc, &runtime)?;
    let start = Instant::now();
    let mut last = "";
    let result = loop {
        if last != job.stage() {
            last = job.stage();
            println!("{last}");
        }
        if let Some(c) = job.poll()? {
            break c;
        }
        if start.elapsed() > Duration::from_secs(600) {
            job.cancel();
            return Err("Hires timeout".into());
        }
        std::thread::sleep(Duration::from_millis(30));
    };
    assert_eq!(
        [result.raster.width(), result.raster.height()],
        [target, target]
    );
    assert_eq!(result.request, request);
    assert_eq!(doc, before);
    assert_eq!(result.source, before.flatten());
    result
        .raster
        .save_png(root.join("artifacts/hires-ultrasharp-img2img.png"))?;
    let saved = Document::from_generated(result.raster, result.request)?;
    let path = root.join("artifacts/hires-ultrasharp-img2img.psd");
    saved.save_project(&path)?;
    assert_eq!(Document::open_project(path)?, saved);
    runtime
        .release_idle_async()
        .recv_timeout(Duration::from_secs(120))??;
    assert!(runtime.residents().is_empty());
    println!("{base} → UltraSharp {target} → Img2Img {target}, PSD recipe, unchanged source and final release PASS: {:?}",start.elapsed());
    Ok(())
}
