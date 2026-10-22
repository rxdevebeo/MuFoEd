//! Re-export independent fixtures and prepare a native layer-mask preview.
use image_editor::{Document, LayerBlendMode, Raster};
use std::path::PathBuf;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let out = PathBuf::from(
        args.next()
            .ok_or("Usage: layer_psd_smoke OUTPUT_DIR [PHOTO.png]")?,
    );
    std::fs::create_dir_all(&out)?;
    for case in [
        "normal", "multiply", "screen", "overlay", "disabled", "offset",
    ] {
        let doc =
            Document::open_project(format!("image-editor/tests/fixtures/psd/layer-{case}.psd"))?;
        let path = out.join(format!("layer-{case}.psd"));
        doc.save_project(&path)?;
        assert_eq!(Document::open_project(&path)?, doc);
        doc.flatten()
            .save_png(out.join(format!("layer-{case}.png")))?;
    }
    if let Some(photo) = args.next() {
        let mut doc = Document::from_raster(Raster::open(photo)?)?;
        doc.add_layer(
            "Свет · Screen".into(),
            Raster::solid(doc.width(), doc.height(), [50, 90, 130, 255])?,
        )?;
        doc.set_blend_mode(LayerBlendMode::Screen)?;
        doc.set_layer("Свет · Screen".into(), true, 170);
        doc.add_layer_mask(true)?;
        let mut pixels = vec![0; doc.width() as usize * doc.height() as usize];
        for y in 0..doc.height() {
            for x in 0..doc.width() {
                pixels[y as usize * doc.width() as usize + x as usize] =
                    u8::try_from(x * 255 / doc.width().max(1)).expect("gradient");
            }
        }
        doc.set_layer_mask(pixels)?;
        doc.save_project(out.join("preview.psd"))?;
    }
    println!("PSD re-export and roundtrip PASS");
    Ok(())
}
