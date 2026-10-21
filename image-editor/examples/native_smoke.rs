//! Real in-process Plakat smoke test. Does not invoke plakat.exe.
use image_editor::{
    generation::{Mode, NativeJob, Request},
    Document, Raster, Tool,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let cache = std::path::absolute(
        args.next()
            .ok_or("Usage: native_smoke CACHE OUTPUT [INPUT]")?,
    )?;
    let output = args.next().ok_or("Missing output path")?;
    // Configure the upstream runtime before starting any host worker threads.
    std::env::set_var("PLAKAT_CACHE_DIR", &cache);
    std::env::set_var("PLAKAT_NO_SDPA", "1");
    let input = args.next();
    let mut doc = if let Some(path) = &input {
        Document::from_raster(Raster::open(path)?)?
    } else {
        Document::new(512, 512)?
    };
    if input.is_some() {
        doc.stroke([200.0, 200.0], [300.0, 300.0], 60.0, [0; 4], Tool::Mask)?;
    }
    let mut request = Request {
        sampler: image_editor::generation::Sampler::Dpmpp2mKarras,
        family: Some(image_editor::assets::Family::Sdxl),
        cache_dir: Some(cache),
        model: "local/image-studio-sdxl".into(),
        mode: if input.is_some() && std::env::var_os("IMAGE_STUDIO_SMOKE_IMG2IMG").is_some() {
            Mode::ImageToImage
        } else if input.is_some() {
            Mode::Inpaint
        } else {
            Mode::TextToImage
        },
        prompt: "a blue ceramic vase on a wooden table, studio still life, soft light".into(),
        steps: 12,
        ..Request::default()
    };
    configure_request(&mut request)?;
    let manager = mufoed_model_runtime::Runtime::global();
    #[cfg(feature = "ollama")]
    managed_text_if_enabled(&manager)?;
    let mut job = check_queue_and_start(request, &doc, &manager)?;
    let start = std::time::Instant::now();
    loop {
        if let Some(candidate) = job.poll()? {
            assert!(manager.snapshot().active.is_none());
            assert!(manager.snapshot().queued.is_empty());
            if candidate.request.mode == Mode::Inpaint {
                for ((out, source), mask) in candidate
                    .raster
                    .pixels()
                    .chunks_exact(4)
                    .zip(doc.flatten().pixels().chunks_exact(4))
                    .zip(doc.mask())
                {
                    if if candidate.request.mask_invert {
                        *mask == 255
                    } else {
                        *mask == 0
                    } {
                        assert_eq!(out, source, "Protected pixel changed");
                    }
                }
            }
            candidate.raster.save_png(output)?;
            println!(
                "In-process Plakat completed in {:.1}s",
                start.elapsed().as_secs_f64()
            );
            #[cfg(feature = "ollama")]
            managed_text_if_enabled(&manager)?;
            return Ok(());
        }
        if start.elapsed().as_secs() > 600 {
            job.cancel();
            return Err("Native inference timed out; process exit stops the worker".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn configure_request(request: &mut Request) -> Result<(), Box<dyn std::error::Error>> {
    if let Ok(model) = std::env::var("IMAGE_STUDIO_SMOKE_MODEL") {
        request.model = model;
    }
    if let Ok(family) = std::env::var("IMAGE_STUDIO_SMOKE_FAMILY") {
        request.family = Some(serde_json::from_value(serde_json::Value::String(family))?);
    }
    if let Ok(prompt) = std::env::var("IMAGE_STUDIO_SMOKE_PROMPT") {
        request.prompt = prompt;
    }
    if let Ok(steps) = std::env::var("IMAGE_STUDIO_SMOKE_STEPS") {
        request.steps = steps.parse()?;
    }
    if std::env::var_os("IMAGE_STUDIO_SMOKE_1024").is_some() {
        request.output_size = Some([1024, 1024]);
    }
    if let Ok(loras) = std::env::var("IMAGE_STUDIO_SMOKE_LORAS") {
        request.loras = serde_json::from_str(&loras)?;
    }
    if let Some(path) = std::env::var_os("IMAGE_STUDIO_SMOKE_LORA") {
        request.loras.push(image_editor::generation::Lora {
            path: path.into(),
            weight: 0.65,
            enabled: true,
        });
    }
    if let Some(path) = std::env::var_os("IMAGE_STUDIO_SMOKE_VAE") {
        request.vae = Some(path.into());
    }
    if std::env::var_os("IMAGE_STUDIO_SMOKE_WIDE").is_some() {
        request.output_size = Some([640, 384]);
    }
    if std::env::var_os("IMAGE_STUDIO_SMOKE_INVERT").is_some() {
        request.mask_invert = true;
        request.mask_feather = 8;
    }
    Ok(())
}

fn check_queue_and_start(
    request: Request,
    doc: &Document,
    manager: &mufoed_model_runtime::Runtime,
) -> Result<NativeJob, Box<dyn std::error::Error>> {
    let mut task = mufoed_model_runtime::Task::new("text", "simulated-host-task");
    task.device = "cpu".into();
    task.memory = mufoed_model_runtime::Memory {
        ram: Some(0),
        vram: Some(0),
    };
    let text = manager.enqueue(task)?;
    let text_lease = text.wait(
        &std::sync::atomic::AtomicBool::new(false),
        std::time::Duration::from_secs(120),
    )?;
    let mut queued = NativeJob::start_with_runtime(request.clone(), doc, manager)?;
    assert!(queued.poll()?.is_none());
    queued.cancel();
    drop(queued);
    let cancelled_at = std::time::Instant::now();
    while NativeJob::is_busy() {
        if cancelled_at.elapsed().as_secs() > 5 {
            return Err("Queued cancellation did not release native guard".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(manager.snapshot().queued.is_empty());
    assert_eq!(
        manager.snapshot().active.as_ref().map(|t| t.owner.as_str()),
        Some("text")
    );
    let mut job = NativeJob::start_with_runtime(request, doc, manager)?;
    assert!(job.poll()?.is_none());
    drop(text_lease);
    Ok(job)
}

#[cfg(feature = "ollama")]
fn managed_text_if_enabled(
    manager: &mufoed_model_runtime::Runtime,
) -> Result<(), Box<dyn std::error::Error>> {
    let Ok(model) = std::env::var("IMAGE_STUDIO_SMOKE_OLLAMA") else {
        return Ok(());
    };
    let started = std::time::Instant::now();
    let endpoint = "http://127.0.0.1:11434";
    let client = mufoed_model_runtime::ollama::Client::new(endpoint, &model, manager)?;
    let reply = client.run("text", &std::sync::atomic::AtomicBool::new(false), || {
        let http = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(180))
            .build()
            .map_err(|e| e.to_string())?;
        let response = http
            .post(format!("{endpoint}/api/chat"))
            .json(&serde_json::json!({
                "model": model, "stream": false, "keep_alive": 0, "think": false,
                "messages": [{"role": "user", "content": "Reply with the single word READY."}],
                "options": {"num_ctx": 4096, "num_predict": 16}
            }))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|e| e.to_string())?;
        let value: serde_json::Value = response.json().map_err(|e| e.to_string())?;
        if value["done"] != true {
            return Err("Non-terminal Ollama response".into());
        }
        Ok(value["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_owned())
    })?;
    if reply.trim().is_empty() {
        return Err("Empty text response".into());
    }
    assert!(manager.snapshot().active.is_none());
    println!(
        "Managed text completed and unload confirmed in {:.1}s: {}",
        started.elapsed().as_secs_f64(),
        reply.trim()
    );
    Ok(())
}
