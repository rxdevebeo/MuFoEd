//! Real CUDA generation/refinement/UltraSharp-1x acceptance, using prepared local weights.
use image_editor::{
    generation::{NativeJob, PipelineJob, Postprocess, Request},
    Document, Raster,
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
    let doc = Document::new(512, 512)?;
    let request = Request {
        model: "local/image-studio-sdxl".into(),
        family: Some(image_editor::assets::Family::Sdxl),
        cache_dir: Some(cache),
        prompt: "a blue ceramic vase on a wooden table, studio still life, soft light".into(),
        steps: 20,
        ..Request::default()
    };
    let mut baseline = NativeJob::start_with_runtime(request.clone(), &doc, &runtime)?;
    let began = Instant::now();
    let initial = loop {
        if let Some(c) = baseline.poll()? {
            break c.raster;
        }
        if began.elapsed() > Duration::from_secs(180) {
            return Err("baseline timeout".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    initial.save_png(root.join("artifacts/postprocess-before.png"))?;
    let mut request = request;
    request.postprocess = Some(Postprocess {
        ultrasharp: root.join(".runtime/Upscale/ultrasharp/4x-UltraSharp.safetensors"),
        ..Postprocess::default()
    });
    let mut job = PipelineJob::start_with_runtime(true, "", request.clone(), &doc, &runtime)?;
    let began = Instant::now();
    let mut last = "";
    let result = loop {
        if job.stage() != last {
            last = job.stage();
            println!("{last}");
        }
        if let Some(c) = job.poll()? {
            break c;
        }
        if began.elapsed() > Duration::from_secs(300) {
            return Err("pipeline timeout".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(result.request, request);
    assert_eq!(result.source, doc.flatten());
    assert_eq!((result.raster.width(), result.raster.height()), (512, 512));
    assert_ne!(result.raster, initial);
    result
        .raster
        .save_png(root.join("artifacts/postprocess-after.png"))?;
    let project = Document::from_generated(result.raster, result.request)?;
    let path = root.join("artifacts/postprocess-result.psd");
    project.save_project(&path)?;
    assert_eq!(Document::open_project(path)?, project);
    println!(
        "Full 512x512 chain / unchanged dimensions / effect / PSD recipe PASS: {:?}",
        began.elapsed()
    );
    check_rgba(&runtime, &root)?;
    runtime
        .release_idle_async()
        .recv_timeout(Duration::from_secs(120))??;
    assert!(runtime.residents().is_empty());
    println!("Final model release PASS");
    Ok(())
}

fn check_rgba(
    runtime: &mufoed_model_runtime::Runtime,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // Independently verify tiled same-size output and exact alpha preservation on RGBA.
    let rgba = Raster::solid(257, 129, [30, 90, 150, 123])?;
    let args = [
        "upscale".into(),
        "--variant".into(),
        "ultrasharp".into(),
        "--model".into(),
        root.join(".runtime/Upscale/ultrasharp/4x-UltraSharp.safetensors")
            .into_os_string(),
        "--device".into(),
        "auto".into(),
        "--tile".into(),
        "128".into(),
        "--keep-size".into(),
    ];
    let mut sharpen = image_editor::model_tools::ModelJob::start_with_runtime(
        "upscale_tool.py",
        &args,
        Some(&rgba),
        None,
        runtime,
    )?;
    loop {
        if let Some(output) = sharpen.poll()? {
            let raster = output.raster.ok_or("missing RGBA output")?;
            assert_eq!((raster.width(), raster.height()), (257, 129));
            assert!(raster.pixels().chunks_exact(4).all(|p| p[3] == 123));
            println!(
                "Same-size multi-tile RGBA / exact alpha PASS: {}",
                output.log
            );
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}
