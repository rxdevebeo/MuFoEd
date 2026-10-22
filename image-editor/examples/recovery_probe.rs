//! Isolated crash-recovery fixture and exact checkpoint verification; no model inference.
use image_editor::{recovery, Document, LayerBlendMode, Raster};
use std::path::PathBuf;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let mode = args
        .next()
        .ok_or("Usage: recovery_probe seed OUTPUT_DIR [PHOTO] | verify RECOVERY_DIR SOURCE.psd")?;
    let directory = PathBuf::from(args.next().ok_or("Missing directory")?);
    if mode == "seed" {
        std::fs::create_dir_all(&directory)?;
        let raster = if let Some(path) = args.next() {
            Raster::open(path)?
        } else {
            Raster::solid(96, 64, [80, 110, 160, 255])?
        };
        let mut doc = Document::from_generated(
            raster,
            image_editor::generation::Request {
                prompt: "recovery vase fixture".into(),
                seed: 987_654,
                ..Default::default()
            },
        )?;
        doc.add_layer(
            "Свет · Screen".into(),
            Raster::solid(doc.width(), doc.height(), [50, 90, 130, 128])?,
        )?;
        doc.set_blend_mode(LayerBlendMode::Screen)?;
        doc.add_layer_mask(true)?;
        let n = doc.width() as usize * doc.height() as usize;
        let coverage = (0..n)
            .map(|i| u8::try_from(i % 256).expect("gradient"))
            .collect();
        doc.set_layer_mask(coverage)?;
        doc.set_layer("Свет · Screen".into(), true, 173);
        doc.set_layer_locked(true);
        doc.set_mask(vec![77; n])?;
        let width = doc.width() as usize;
        let height = doc.height() as usize;
        doc.set_selection(
            (0..n)
                .map(|i| {
                    let (x, y) = (i % width, i / width);
                    if x >= width / 4 && x < width * 3 / 4 && y >= height / 4 && y < height * 3 / 4
                    {
                        128
                    } else {
                        0
                    }
                })
                .collect(),
        )?;
        doc.save_project(directory.join("original.psd"))?;
        std::fs::write(
            directory.join("settings.json"),
            serde_json::to_vec_pretty(&image_editor::ui::ImageEditor::default().settings())?,
        )?;
        println!("Recovery fixture prepared: {}", directory.display());
    } else if mode == "verify" {
        let source = PathBuf::from(args.next().ok_or("Missing original PSD")?);
        let expected = Document::open_project(&source)?;
        let candidates = recovery::discover(&directory)?;
        assert_eq!(candidates.len(), 1, "Expected one abandoned checkpoint");
        let draft = candidates[0].load()?;
        assert_eq!(draft.document, expected);
        assert_eq!(draft.source.as_deref(), Some(source.as_path()));
        println!(
            "Forced-process-exit recovery: exact pixels/layers/masks/selection/recipe/origin PASS"
        );
    } else {
        return Err("Unknown recovery probe mode".into());
    }
    Ok(())
}
