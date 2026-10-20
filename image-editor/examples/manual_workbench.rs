//! Reproducible layered fixture for manual-workbench review (no model required).
use image_editor::{BrushStyle, Document, Raster, Shape};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut doc = Document::new(960, 640)?;
    doc.gradient(
        [0.0, 0.0],
        [960.0, 640.0],
        [24, 39, 65, 255],
        [78, 111, 148, 255],
        BrushStyle::default(),
    )?;
    doc.set_layer("Фон · вечерний синий".into(), true, 255);
    doc.set_layer_locked(true);
    doc.add_layer("Свет".into(), Raster::solid(960, 640, [0; 4])?)?;
    doc.draw_shape(
        [180.0, 60.0],
        [780.0, 610.0],
        Shape::Ellipse,
        1.0,
        true,
        [184, 210, 238, 70],
        BrushStyle::default(),
    )?;
    let cutout = Raster::open(root.join("artifacts/bridge-birefnet.png"))?;
    let source =
        image::RgbaImage::from_raw(cutout.width(), cutout.height(), cutout.pixels().to_vec())
            .unwrap();
    let mut bounds = [source.width(), source.height(), 0, 0];
    for (x, y, p) in source.enumerate_pixels() {
        if p[3] > 10 {
            bounds[0] = bounds[0].min(x);
            bounds[1] = bounds[1].min(y);
            bounds[2] = bounds[2].max(x);
            bounds[3] = bounds[3].max(y);
        }
    }
    let source = image::imageops::crop_imm(
        &source,
        bounds[0],
        bounds[1],
        bounds[2] - bounds[0] + 1,
        bounds[3] - bounds[1] + 1,
    )
    .to_image();
    let source = image::imageops::resize(&source, 270, 340, image::imageops::FilterType::Lanczos3);
    let mut canvas = image::RgbaImage::new(960, 640);
    image::imageops::overlay(&mut canvas, &source, 345, 170);
    doc.add_layer(
        "Ваза · без фона".into(),
        Raster::new(960, 640, canvas.into_raw())?,
    )?;
    doc.add_layer("Контур".into(), Raster::solid(960, 640, [0; 4])?)?;
    doc.draw_shape(
        [48.0, 48.0],
        [912.0, 592.0],
        Shape::Rectangle,
        2.0,
        false,
        [210, 225, 245, 180],
        BrushStyle::default(),
    )?;
    doc.select(2)?;
    doc.save_project(root.join("artifacts/manual-workbench.json"))?;
    doc.flatten()
        .save_png(root.join("artifacts/manual-workbench.png"))?;
    Ok(())
}
