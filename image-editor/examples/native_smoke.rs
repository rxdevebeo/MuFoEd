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
    let manager = mufoed_model_runtime::Runtime::new(mufoed_model_runtime::Policy::default());
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
            return Ok(());
        }
        if start.elapsed().as_secs() > 600 {
            job.cancel();
            return Err("Native inference timed out; process exit stops the worker".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn check_queue_and_start(
    request: Request,
    doc: &Document,
    manager: &mufoed_model_runtime::Runtime,
) -> Result<NativeJob, Box<dyn std::error::Error>> {
    let mut text = manager.enqueue(mufoed_model_runtime::Task::new(
        "text",
        "simulated-host-task",
    ))?;
    let text_lease = text.try_start()?.ok_or("Text admission failed")?;
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
