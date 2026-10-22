//! Real inference smoke test; no synthetic output or mock backend.
use image_editor::{
    generation::{Job, Mode, Request},
    Document, Tool,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let executable = args
        .next()
        .ok_or("Usage: plakat_smoke PLAKAT CACHE OUTPUT [inpaint INPUT]")?;
    let cache = args.next().ok_or("Missing cache")?;
    let output = args.next().ok_or("Missing output PNG")?;
    let inpaint = args.next().is_some();
    let mut doc = if inpaint {
        Document::from_raster(image_editor::Raster::open(
            args.next().ok_or("Missing input PNG")?,
        )?)?
    } else {
        Document::new(512, 512)?
    };
    if inpaint {
        doc.stroke([200.0, 200.0], [300.0, 300.0], 60.0, [0; 4], Tool::Mask)?;
    }
    let request = Request {
        cache_dir: Some(cache.into()),
        model: "local/image-studio-sdxl".into(),
        prompt: if inpaint {
            "a blue ceramic vase on a wooden table"
        } else {
            "a red ceramic vase on a wooden table, studio still life, soft light"
        }
        .into(),
        mode: if inpaint {
            Mode::Inpaint
        } else {
            Mode::TextToImage
        },
        steps: 12,
        ..Request::default()
    };
    let mut job = Job::start(executable, request, &doc)?;
    let start = std::time::Instant::now();
    loop {
        if let Some(candidate) = job.poll()? {
            if inpaint {
                for ((result, source), mask) in candidate
                    .raster
                    .pixels()
                    .chunks_exact(4)
                    .zip(doc.flatten().pixels().chunks_exact(4))
                    .zip(doc.mask())
                {
                    if *mask == 0 {
                        assert_eq!(result, source, "inpaint changed a protected pixel");
                    }
                }
            }
            candidate.raster.save_png(output)?;
            println!(
                "Real Plakat inference completed in {:.1}s",
                start.elapsed().as_secs_f64()
            );
            return Ok(());
        }
        if start.elapsed().as_secs() > 600 {
            job.cancel();
            return Err("Inference timed out after 600s".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
