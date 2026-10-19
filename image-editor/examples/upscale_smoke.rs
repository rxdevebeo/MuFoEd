//! End-to-end upscale through the same Rust/Python bridge as the editor.
use image_editor::{model_tools::ModelJob, Raster};
use std::{
    ffi::OsString,
    path::PathBuf,
    time::{Duration, Instant},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(
        args.next()
            .ok_or("Usage: upscale_smoke RUNTIME INPUT OUTPUT")?,
    );
    let source = Raster::open(args.next().ok_or("Missing input")?)?;
    let output = args.next().ok_or("Missing output")?;
    let parameters: Vec<OsString> = vec![
        "upscale".into(),
        "--variant".into(),
        "blend".into(),
        "--model".into(),
        root.join("Upscale/ultrasharp/4x-UltraSharp.safetensors")
            .into_os_string(),
        "--aura-model".into(),
        root.join("Upscale/aurasr").into_os_string(),
        "--ultrasharp-weight".into(),
        "0.65".into(),
        "--aura-weight".into(),
        "0.35".into(),
        "--device".into(),
        "auto".into(),
    ];
    let mut job = ModelJob::start("upscale_tool.py", &parameters, Some(&source), None)?;
    let start = Instant::now();
    loop {
        if let Some(result) = job.poll()? {
            let raster = result.raster.ok_or("No upscale PNG")?;
            assert_eq!(
                [raster.width(), raster.height()],
                [source.width() * 4, source.height() * 4]
            );
            if source.pixels().chunks_exact(4).any(|p| p[3] == 0) {
                assert!(raster.pixels().chunks_exact(4).any(|p| p[3] == 0));
            }
            if source.pixels().chunks_exact(4).any(|p| p[3] == 255) {
                assert!(raster.pixels().chunks_exact(4).any(|p| p[3] == 255));
            }
            raster.save_png(output)?;
            println!(
                "Rust bridge 65/35 upscale: {:.1}s, {}x{}, alpha retained",
                start.elapsed().as_secs_f64(),
                raster.width(),
                raster.height()
            );
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(300) {
            return Err("Upscale timed out".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
