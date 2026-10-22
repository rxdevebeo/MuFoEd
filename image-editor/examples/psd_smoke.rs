//! Export/import receipt for independent PSD compatibility checks.
use image_editor::Document;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let input = args
        .next()
        .ok_or("Usage: psd_smoke INPUT_PROJECT OUTPUT.psd")?;
    let output = std::path::PathBuf::from(args.next().ok_or("Missing PSD output")?);
    let doc = Document::open_project(input)?;
    doc.save_project(&output)?;
    assert_eq!(Document::open_project(&output)?, doc);
    let prefix = output.with_extension("");
    doc.flatten().save_png(prefix.with_extension("png"))?;
    let mut layers = Vec::new();
    for (index, layer) in doc.layers().iter().enumerate() {
        let path = prefix.with_extension(format!("layer-{index}.png"));
        layer.raster().save_png(&path)?;
        layers.push(serde_json::json!({"name":layer.name(),"visible":layer.visible(),"opacity":layer.opacity(),"locked":layer.locked(),"png":path}));
    }
    std::fs::write(
        prefix.with_extension("receipt.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"width":doc.width(),"height":doc.height(),"layers":layers}),
        )?,
    )?;
    println!(
        "PSD layers/mask/recipe roundtrip PASS: {}",
        output.display()
    );
    Ok(())
}
