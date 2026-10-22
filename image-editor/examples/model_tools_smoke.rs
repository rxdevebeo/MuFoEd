//! Test the same subprocess bridge used by the UI, with already downloaded weights.
use image_editor::{model_tools::ModelJob, Raster};
use mufoed_model_runtime::{Policy, Runtime, Task};
use std::{
    ffi::OsString,
    path::PathBuf,
    time::{Duration, Instant},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(
        args.next()
            .ok_or("Usage: model_tools_smoke RUNTIME INPUT OUTPUT_DIR")?,
    );
    let source = Raster::open(args.next().ok_or("Missing input")?)?;
    let output = PathBuf::from(args.next().ok_or("Missing output directory")?);
    let runtime = Runtime::new(Policy::default());
    for (script, parameters, name) in [
        (
            "birefnet.py",
            vec![
                OsString::from("remove"),
                "--model".into(),
                root.join("BiRefNet/tiny.onnx").into_os_string(),
                "--device".into(),
                "cpu".into(),
            ],
            "bridge-birefnet.png",
        ),
        (
            "sam2_tool.py",
            vec![
                OsString::from("segment"),
                "--model".into(),
                root.join("SAM2/sam2.1_hiera_large").into_os_string(),
                "--device".into(),
                "auto".into(),
                "--points".into(),
                "[[340,70,1],[200,300,0]]".into(),
            ],
            "bridge-sam21-mask.png",
        ),
    ] {
        // Simulate a host text adapter holding admission; real image inference must wait.
        let mut text = runtime.enqueue(Task::new("text", "simulated-host-task"))?;
        let text_lease = text.try_start()?.ok_or("Text admission failed")?;
        let mut job =
            ModelJob::start_with_runtime(script, &parameters, Some(&source), None, &runtime)?;
        assert!(job.poll()?.is_none());
        assert_eq!(runtime.snapshot().queued.len(), 1);
        drop(text_lease);
        let start = Instant::now();
        loop {
            if let Some(result) = job.poll()? {
                assert!(runtime.snapshot().active.is_none());
                assert!(runtime.snapshot().queued.is_empty());
                let raster = result.raster.ok_or("No result PNG")?;
                assert_eq!(
                    [raster.width(), raster.height()],
                    [source.width(), source.height()]
                );
                if script == "birefnet.py" {
                    assert!(raster.pixels().chunks_exact(4).any(|p| p[3] < 10));
                    assert!(raster.pixels().chunks_exact(4).any(|p| p[3] > 240));
                    for (out, input) in raster
                        .pixels()
                        .chunks_exact(4)
                        .zip(source.pixels().chunks_exact(4))
                    {
                        assert_eq!(&out[..3], &input[..3]);
                        assert!(out[3] <= input[3]);
                    }
                } else {
                    assert!(raster.pixels().chunks_exact(4).any(|p| p[0] == 255));
                    assert!(raster.pixels().chunks_exact(4).any(|p| p[0] == 0));
                }
                raster.save_png(output.join(name))?;
                println!(
                    "{script}: {:.1}s; source size and pixel contract passed",
                    start.elapsed().as_secs_f64()
                );
                break;
            }
            if start.elapsed() > Duration::from_secs(180) {
                return Err("Model tool timed out".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    Ok(())
}
