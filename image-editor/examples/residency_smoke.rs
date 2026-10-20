//! Real R03-R05 mixed lifecycle and warm-cache receipt. Uses only prepared local weights.
use image_editor::{
    generation::{NativeJob, Request},
    model_tools::ModelJob,
    Document, Raster,
};
use mufoed_model_runtime::{
    telemetry::{SystemTelemetry, Telemetry},
    Runtime,
};
use std::{
    ffi::OsString,
    path::PathBuf,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
fn record(runtime: &Runtime, label: &str) {
    let o = SystemTelemetry.observe();
    println!(
        "{label}: RAM {:?}/{:?}; GPUs {:?}; residents {:?}",
        o.ram_available,
        o.ram_total,
        o.gpus,
        runtime.residents()
    );
}
#[cfg(feature = "ollama")]
fn text(runtime: &Runtime) -> Result<(), Box<dyn std::error::Error>> {
    let client = mufoed_model_runtime::ollama::Client::new(
        "http://localhost:11434",
        "llama3:latest",
        runtime,
    )?
    .with_context(8192);
    let began = Instant::now();
    for _ in 0..2 {
        let result=client.run("text",&AtomicBool::new(false),|| {
            reqwest::blocking::Client::new().post("http://localhost:11434/api/chat").json(&serde_json::json!({"model":"llama3:latest","stream":false,"keep_alive":300,"messages":[{"role":"user","content":"Reply with one word: ready"}],"options":{"num_ctx":8192,"num_predict":8,"seed":42,"temperature":0}})).send().map_err(|e|e.to_string())?.json::<serde_json::Value>().map_err(|e|e.to_string())
        })?;
        println!(
            "text result={}, load_duration={}",
            result["message"]["content"], result["load_duration"]
        );
    }
    println!("text pair elapsed={:?}", began.elapsed());
    Ok(())
}
fn model(
    runtime: &Runtime,
    script: &str,
    args: &[OsString],
    source: &Raster,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut job = ModelJob::start_with_runtime(script, args, Some(source), None, runtime)?;
    let began = Instant::now();
    loop {
        if let Some(output) = job.poll()? {
            println!("{script}: {:?}; {}", began.elapsed(), output.log);
            return Ok(output.log);
        }
        if began.elapsed() > Duration::from_secs(600) {
            return Err("model timeout".into());
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}
fn native(
    runtime: &Runtime,
    request: Request,
    doc: &Document,
) -> Result<Raster, Box<dyn std::error::Error>> {
    let mut job = NativeJob::start_with_runtime(request, doc, runtime)?;
    let began = Instant::now();
    loop {
        if let Some(candidate) = job.poll()? {
            return Ok(candidate.raster);
        }
        if began.elapsed() > Duration::from_secs(600) {
            return Err("native timeout".into());
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}
fn transforms(
    runtime: &Runtime,
    request: &Request,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use image_editor::{generation::Mode, Tool};
    let source = Raster::open(root.join("artifacts/native-generated-vase.png"))?;
    let mut doc = Document::from_raster(source.clone())?;
    doc.stroke([220.0, 220.0], [280.0, 280.0], 40.0, [0; 4], Tool::Mask)?;
    let original = doc.clone();
    let mut request = request.clone();
    request.mode = Mode::ImageToImage;
    request.strength = 0.0;
    assert_eq!(native(runtime, request.clone(), &doc)?, source);
    request.strength = 0.7;
    request.prompt = "a red ceramic vase on a wooden table, studio still life".into();
    let image = native(runtime, request.clone(), &doc)?;
    assert_ne!(image, source);
    image.save_png(root.join("artifacts/resident-img2img.png"))?;
    request.mode = Mode::Inpaint;
    request.mask_feather = 3;
    let image = native(runtime, request.clone(), &doc)?;
    for ((actual, expected), mask) in image
        .pixels()
        .chunks_exact(4)
        .zip(source.pixels().chunks_exact(4))
        .zip(doc.mask())
    {
        if *mask == 0 {
            assert_eq!(actual, expected);
        }
    }
    image.save_png(root.join("artifacts/resident-inpaint.png"))?;
    assert_eq!(doc, original);
    println!("real img2img / inpaint / zero-strength / source preservation PASS");
    let lora = std::path::Path::new("D:/Models/add-detail-xl.safetensors");
    if lora.is_file() {
        request.mode = Mode::TextToImage;
        request.steps = 2;
        request.loras.push(image_editor::generation::Lora {
            path: lora.into(),
            enabled: true,
            weight: 0.2,
        });
        native(runtime, request.clone(), &doc)?
            .save_png(root.join("artifacts/resident-lora-02.png"))?;
        request.loras[0].weight = 0.4;
        native(runtime, request.clone(), &doc)?
            .save_png(root.join("artifacts/resident-lora-04.png"))?;
        println!("real LoRA scale switch 0.2 → 0.4 PASS");
    }
    // Cancel active work only after its UNet stage is observed; no UI or document mutation.
    request.loras.clear();
    request.steps = 150;
    let mut job = NativeJob::start_with_runtime(request, &doc, runtime)?;
    let began = Instant::now();
    let mut cancelled = false;
    loop {
        if !cancelled
            && runtime.snapshot().active.is_some()
            && runtime
                .events()
                .last()
                .is_some_and(|e| e.detail == "UNet denoise" && e.at >= began)
        {
            job.cancel();
            cancelled = true;
        }
        match job.poll() {
            Err(_) if cancelled => break,
            Ok(None) => {}
            Err(e) => return Err(e.into()),
            Ok(Some(_)) => return Err("cancellation did not precede completion".into()),
        }
        if began.elapsed() > Duration::from_secs(180) {
            return Err("active cancellation timeout".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    while NativeJob::is_busy() {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(doc, original);
    assert!(runtime.snapshot().active.is_none());
    println!("real active native cancellation / queue release PASS");
    Ok(())
}
#[allow(clippy::too_many_lines)] // Ordered mixed-backend acceptance receipt.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cache = root.join(".runtime/hub");
    std::env::set_var("PLAKAT_CACHE_DIR", &cache);
    std::env::set_var("PLAKAT_NO_SDPA", "1");
    let runtime = Runtime::global();
    record(&runtime, "baseline");
    #[cfg(feature = "ollama")]
    text(&runtime)?;
    record(&runtime, "after text warm");
    let doc = Document::new(512, 512)?;
    let request = Request {
        model: "local/image-studio-sdxl".into(),
        family: Some(image_editor::assets::Family::Sdxl),
        cache_dir: Some(cache),
        prompt: "a blue ceramic vase on a wooden table, studio still life, soft light".into(),
        steps: 4,
        ..Request::default()
    };
    if let Some(vae) = std::env::var_os("IMAGE_STUDIO_RESIDENCY_VAE") {
        let baseline = native(&runtime, request.clone(), &doc)?;
        let mut alternate = request.clone();
        alternate.vae = Some(vae.into());
        match native(&runtime, alternate, &doc) {
            Ok(image) => {
                image.save_png(root.join("artifacts/resident-alternate-vae.png"))?;
                println!("real alternate VAE inference PASS");
            }
            Err(error) if error.to_string().contains("VAE produced non-finite") => {
                println!("real alternate VAE rejected non-finite output PASS");
            }
            Err(error) => return Err(error),
        }
        assert_eq!(native(&runtime, request, &doc)?, baseline);
        runtime
            .release_idle_async()
            .recv_timeout(Duration::from_secs(120))??;
        assert!(runtime.residents().is_empty());
        println!("real VAE switch / default restoration / release PASS");
        return Ok(());
    }
    for n in 0..2 {
        let began = Instant::now();
        let image = native(&runtime, request.clone(), &doc)?;
        image.save_png(root.join(format!("artifacts/resident-native-{n}.png")))?;
        println!("native {n}: {:?}", began.elapsed());
        record(&runtime, &format!("after native {n}"));
    }
    assert_eq!(
        Raster::open(root.join("artifacts/resident-native-0.png"))?,
        Raster::open(root.join("artifacts/resident-native-1.png"))?
    );
    let mut large = request.clone();
    large.output_size = Some([1024, 1024]);
    large.steps = 2;
    let began = Instant::now();
    native(&runtime, large, &doc)?.save_png(root.join("artifacts/resident-1024.png"))?;
    println!("real 1024x1024: {:?}", began.elapsed());
    transforms(&runtime, &request, &root)?;
    let source = Raster::open(root.join("artifacts/native-generated-vase.png"))?;
    let models = root.join(".runtime");
    for points in ["[[340,70,1],[200,300,0]]", "[[340,72,1],[200,300,0]]"] {
        model(
            &runtime,
            "sam2_tool.py",
            &[
                "segment".into(),
                "--model".into(),
                models.join("SAM2/sam2.1_hiera_large").into_os_string(),
                "--device".into(),
                "auto".into(),
                "--points".into(),
                points.into(),
            ],
            &source,
        )?;
    }
    record(&runtime, "after SAM warm/embeddings");
    model(
        &runtime,
        "birefnet.py",
        &[
            "remove".into(),
            "--model".into(),
            models.join("BiRefNet/tiny.onnx").into_os_string(),
            "--device".into(),
            "cpu".into(),
        ],
        &source,
    )?;
    let small = Raster::solid(32, 32, [60, 100, 150, 180])?;
    for _ in 0..2 {
        model(
            &runtime,
            "upscale_tool.py",
            &[
                "upscale".into(),
                "--variant".into(),
                "blend".into(),
                "--model".into(),
                models
                    .join("Upscale/ultrasharp/4x-UltraSharp.safetensors")
                    .into_os_string(),
                "--aura-model".into(),
                models.join("Upscale/aurasr").into_os_string(),
                "--device".into(),
                "auto".into(),
                "--ultrasharp-weight".into(),
                "0.65".into(),
                "--aura-weight".into(),
                "0.35".into(),
            ],
            &small,
        )?;
    }
    record(&runtime, "after upscale warm");
    #[cfg(feature = "ollama")]
    text(&runtime)?;
    record(&runtime, "after return to text");
    runtime
        .release_idle_async()
        .recv_timeout(Duration::from_secs(120))??;
    record(&runtime, "after explicit release");
    assert!(runtime.residents().is_empty());
    assert!(runtime.snapshot().active.is_none());
    println!("events={:?}", runtime.events());
    // A real low-headroom preflight, without allocating an enormous tensor.
    let mut task = mufoed_model_runtime::Task::new("acceptance", "physical over-budget rejection");
    task.memory = mufoed_model_runtime::Memory {
        ram: Some(0),
        vram: Some(64 << 30),
    };
    let rejected = runtime
        .enqueue(task)?
        .wait(&AtomicBool::new(false), Duration::from_secs(10));
    assert!(matches!(
        rejected,
        Err(mufoed_model_runtime::Error::OverBudget { .. })
    ));
    println!("real physical memory rejection PASS");
    Ok(())
}
