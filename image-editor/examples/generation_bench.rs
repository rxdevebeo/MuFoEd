//! Repeatable end-to-end CUDA baseline. Cold runtime cache, then two warm runs.
use image_editor::{
    generation::{HiresFix, PipelineJob, Postprocess, Request, Sampler},
    Document,
};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
fn request(root: &Path) -> Result<Request, Box<dyn std::error::Error>> {
    let base = std::env::var("HIRES_SMOKE_BASE")
        .unwrap_or_else(|_| "1024".into())
        .parse::<u32>()?;
    let refine_steps = std::env::var("BENCH_REFINE_STEPS")
        .ok()
        .map(|v| v.parse::<u32>())
        .transpose()?;
    let target = std::env::var("BENCH_TARGET")
        .ok()
        .map(|v| v.parse::<u32>())
        .transpose()?
        .unwrap_or(base * 3 / 2);
    let steps = std::env::var("BENCH_STEPS")
        .unwrap_or_else(|_| "10".into())
        .parse::<u32>()?;
    let mut request = Request {
        model: "local/image-studio-sdxl".into(),
        family: Some(image_editor::assets::Family::Sdxl),
        cache_dir: Some(root.join(".runtime/hub")),
        output_size: Some([base, base]),
        prompt: "a blue ceramic vase on a wooden table, studio still life, soft light".into(),
        sampler: Sampler::Dpmpp2mKarras,
        steps,
        seed: 42,
        postprocess: Some(Postprocess {
            hires: Some(HiresFix {
                target_size: [target; 2],
                guidance: std::env::var("BENCH_REFINE_CFG")
                    .unwrap_or_else(|_| "6.0".into())
                    .parse::<f32>()?,
            }),
            denoise: 0.3,
            steps: refine_steps.unwrap_or(steps),
            ultrasharp: root.join(".runtime/Upscale/ultrasharp/4x-UltraSharp.safetensors"),
            ..Default::default()
        }),
        ..Default::default()
    };
    if let Ok(model) = std::env::var("BENCH_MODEL") {
        request.model = model;
    }
    if let Ok(family) = std::env::var("BENCH_FAMILY") {
        request.family = Some(serde_json::from_value(serde_json::Value::String(family))?);
    }
    if let Ok(loras) = std::env::var("BENCH_LORAS") {
        request.loras = serde_json::from_str(&loras)?;
    }
    Ok(request)
}
fn run(
    request: &Request,
    doc: &Document,
    runtime: &mufoed_model_runtime::Runtime,
) -> Result<(image_editor::generation::Candidate, f64), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let mut job = PipelineJob::start_with_runtime(true, "", request.clone(), doc, runtime)?;
    let mut last = "";
    loop {
        if last != job.stage() {
            last = job.stage();
            println!("{last}");
        }
        if let Some(value) = job.poll()? {
            return Ok((value, start.elapsed().as_secs_f64()));
        }
        if start.elapsed() > Duration::from_secs(600) {
            job.cancel();
            return Err("Benchmark timeout".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(root.join("artifacts"))?;
    let profile = std::env::var_os("IMAGE_STUDIO_TIMINGS").map_or_else(
        || root.join("artifacts/generation-bench.jsonl"),
        PathBuf::from,
    );
    let diagnostics = std::env::var_os("BENCH_NO_TIMINGS").is_none();
    if diagnostics {
        std::fs::write(&profile, "")?;
        std::env::set_var("IMAGE_STUDIO_TIMINGS", &profile);
    } else {
        std::env::remove_var("IMAGE_STUDIO_TIMINGS");
    }
    let receipt_path = std::env::var_os("BENCH_RECEIPT").map_or_else(
        || root.join("artifacts/generation-bench-receipt.json"),
        PathBuf::from,
    );
    std::fs::write(
        &receipt_path,
        serde_json::to_vec(&serde_json::json!({"status":"running"}))?,
    )?;
    std::env::set_var("PLAKAT_CACHE_DIR", root.join(".runtime/hub"));
    std::env::set_var("PLAKAT_NO_SDPA", "1");
    let request = request(&root)?;
    let [w, h] = request.output_size.ok_or("No benchmark dimensions")?;
    let doc = Document::new(w, h)?;
    let before = doc.clone();
    let runtime = mufoed_model_runtime::Runtime::global();
    assert!(runtime.residents().is_empty());
    let mut rows = Vec::new();
    let runs = std::env::var("BENCH_RUNS")
        .unwrap_or_else(|_| "3".into())
        .parse::<u32>()?;
    if !(1..=10).contains(&runs) {
        return Err("BENCH_RUNS must be 1..10".into());
    }
    for index in 0..runs {
        let label = if index == 0 {
            "cold".to_owned()
        } else {
            format!("warm{index}")
        };
        println!("BENCH START {label}");
        let (result, elapsed) = run(&request, &doc, &runtime)?;
        assert_eq!(result.request, request);
        assert_eq!(doc, before);
        assert_eq!(result.source, before.flatten());
        assert_eq!(
            [result.raster.width(), result.raster.height()],
            request.final_size(&doc)
        );
        let residents: Vec<_> = runtime.residents().into_iter().map(|r| serde_json::json!({"key":r.key,"hits":r.hits,"ram":r.memory.ram,"vram":r.memory.vram})).collect();
        println!("BENCH END {label}: {elapsed:.3}s");
        rows.push(serde_json::json!({"run":label,"elapsed_s":elapsed,"residents":residents,"pixels_sha256":format!("{:x}",Sha256::digest(result.raster.pixels()))}));
        if index + 1 == runs {
            result
                .raster
                .save_png(root.join("artifacts/generation-bench-final.png"))?;
        }
    }
    runtime
        .release_idle_async()
        .recv_timeout(Duration::from_secs(120))??;
    assert!(runtime.residents().is_empty());
    let receipt = serde_json::json!({"status":"completed","request":request,"runs":rows,"timings":if diagnostics {Some(profile)} else {None},
        "diagnostics":diagnostics,"cudnn":cfg!(feature="plakat-cudnn"),"attention_head_slice":2,"cpu_weight_cache_version":2,
        "cache_note":"cold runtime cache, OS file cache retained; warm means reusable CPU/Python cache, not persistent CUDA UNet"});
    std::fs::write(receipt_path, serde_json::to_vec_pretty(&receipt)?)?;
    Ok(())
}
