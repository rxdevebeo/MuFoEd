//! Layer editing preserves transparency, document isolation and undo.
use image_editor::{ColorCorrection, Document, History, Raster};
fn pixel(raster: &Raster, x: usize, y: usize) -> &[u8] {
    let i = (y * raster.width() as usize + x) * 4;
    &raster.pixels()[i..i + 4]
}
#[test]
fn clear_replaces_all_layers_and_mask_with_fresh_document_and_is_undoable() {
    let mut doc = Document::new(3, 3).unwrap();
    doc.add_layer(
        "Keep me".into(),
        Raster::solid(3, 3, [20, 40, 60, 255]).unwrap(),
    )
    .unwrap();
    doc.set_layer("Keep me".into(), false, 123);
    doc.set_layer_locked(true);
    doc.set_mask(vec![190; 9]).unwrap();
    let before = doc.clone();
    let mut history = History::default();
    history.checkpoint(&doc);
    doc.clear_canvas().unwrap();
    assert_eq!(doc.layers().len(), 1);
    assert_eq!(doc, Document::new(3, 3).unwrap());
    assert_eq!(doc.layers()[0].opacity(), 255);
    assert!(doc.layers()[0].raster().pixels().iter().all(|v| *v == 0));
    assert!(doc.mask().iter().all(|v| *v == 0));
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
}
#[test]
fn retain_content_and_locked_layer_protection() {
    let mut doc = Document::new(3, 3).unwrap();
    doc.add_layer(
        "Active".into(),
        Raster::solid(3, 3, [120, 50, 30, 255]).unwrap(),
    )
    .unwrap();
    let layer = doc.layers()[1].clone();
    doc.retain_active_layer().unwrap();
    assert_eq!(doc.layers(), &[layer]);
    doc.set_layer_locked(true);
    let before = doc.clone();
    assert!(doc.correct_color(ColorCorrection::default()).is_err());
    assert!(doc.transform_layer(90.0, [1.0; 2]).is_err());
    assert_eq!(doc, before);
    doc.add_layer("Other".into(), Raster::solid(3, 3, [0; 4]).unwrap())
        .unwrap();
    let before = doc.clone();
    assert!(doc.retain_active_layer().is_err());
    assert_eq!(doc, before);
}
#[test]
fn layer_rotation_keeps_canvas_other_layers_and_mask() {
    let mut doc = Document::new(3, 3).unwrap();
    let mut data = vec![0; 36];
    for (i, v) in [10, 20, 30, 40, 50, 60, 70, 80, 90].iter().enumerate() {
        data[i * 4..i * 4 + 4].copy_from_slice(&[*v, 0, 0, 255]);
    }
    doc.add_layer("Rotate".into(), Raster::new(3, 3, data).unwrap())
        .unwrap();
    doc.set_mask(vec![128; 9]).unwrap();
    let before = doc.clone();
    doc.transform_layer(90.0, [1.0; 2]).unwrap();
    let raster = doc.layers()[1].raster();
    assert_eq!(pixel(raster, 0, 0), [70, 0, 0, 255]);
    assert_eq!(pixel(raster, 2, 0), [10, 0, 0, 255]);
    assert_eq!(doc.layers()[0], before.layers()[0]);
    assert_eq!(doc.mask(), before.mask());
    assert_eq!([doc.width(), doc.height()], [3, 3]);
    let before = doc.clone();
    assert!(doc.transform_layer(f64::NAN, [1.0; 2]).is_err());
    assert!(doc.transform_layer(0.0, [0.0, 1.0]).is_err());
    assert_eq!(doc, before);
}
#[test]
fn scaling_preserves_premultiplied_color_and_clips_without_resizing_canvas() {
    let mut doc = Document::new(3, 3).unwrap();
    let mut data = vec![0; 36];
    data[16..20].copy_from_slice(&[255, 0, 0, 255]);
    doc.add_layer("Scale".into(), Raster::new(3, 3, data).unwrap())
        .unwrap();
    doc.transform_layer(37.0, [2.0; 2]).unwrap();
    for p in doc.layers()[1]
        .raster()
        .pixels()
        .chunks_exact(4)
        .filter(|p| p[3] > 0)
    {
        assert_eq!(&p[..3], [255, 0, 0]);
    }
    assert_eq!([doc.width(), doc.height()], [3, 3]);
}
#[test]
fn import_centers_small_image_and_fits_large_image() {
    let mut doc = Document::new(8, 6).unwrap();
    let bottom = doc.layers()[0].clone();
    let source = Raster::solid(2, 2, [30, 60, 90, 123]).unwrap();
    doc.import_layer("Small".into(), &source).unwrap();
    let raster = doc.layers()[1].raster();
    assert_eq!(pixel(raster, 3, 2), [30, 60, 90, 123]);
    assert_eq!(pixel(raster, 2, 2), [0; 4]);
    assert_eq!(doc.layers()[0], bottom);
    doc.import_layer("Large".into(), &Raster::solid(16, 8, [255; 4]).unwrap())
        .unwrap();
    let raster = doc.layers()[2].raster();
    assert_eq!(pixel(raster, 0, 0), [0; 4]);
    assert_eq!(pixel(raster, 0, 1), [255; 4]);
    assert_eq!(doc.active(), 2);
}
#[test]
fn color_defaults_are_identity_and_corrections_preserve_alpha_and_other_layers() {
    let raster = Raster::new(2, 1, vec![40, 80, 120, 77, 200, 100, 0, 0]).unwrap();
    assert_eq!(ColorCorrection::default().apply(&raster).unwrap(), raster);
    let mut doc = Document::new(2, 1).unwrap();
    doc.add_layer("Photo".into(), raster).unwrap();
    doc.set_mask(vec![120; 2]).unwrap();
    let before = doc.clone();
    doc.correct_color(ColorCorrection {
        saturation: 0.0,
        ..Default::default()
    })
    .unwrap();
    let p = pixel(doc.layers()[1].raster(), 0, 0);
    assert_eq!(p[0], p[1]);
    assert_eq!(p[1], p[2]);
    assert_eq!(p[3], 77);
    assert_eq!(pixel(doc.layers()[1].raster(), 1, 0)[3], 0);
    assert_eq!(doc.layers()[0], before.layers()[0]);
    assert_eq!(doc.mask(), before.mask());
    let before = doc.clone();
    assert!(doc
        .correct_color(ColorCorrection {
            gamma: 0.0,
            ..Default::default()
        })
        .is_err());
    assert_eq!(doc, before);
}

#[test]
fn exposure_uses_linear_light_not_clipped_srgb_multiplication() {
    let source = Raster::solid(1, 1, [128, 128, 128, 45]).unwrap();
    let result = ColorCorrection {
        exposure: 1.0,
        ..Default::default()
    }
    .apply(&source)
    .unwrap();
    assert_eq!(result.pixels(), [176, 176, 176, 45]);
}

#[test]
fn photo_geometry_keeps_mask_other_layers_and_rejects_invalid_coefficients() {
    let mut doc = Document::new(16, 8).unwrap();
    doc.add_layer(
        "Photo".into(),
        Raster::solid(16, 8, [255, 0, 0, 220]).unwrap(),
    )
    .unwrap();
    doc.set_mask(vec![85; 128]).unwrap();
    let original = doc.clone();
    doc.transform_photo_layer(
        0.1,
        [1.0; 2],
        image_editor::PhotoGeometry {
            vertical: 0.2,
            radial: 0.1,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(doc.layers()[0], original.layers()[0]);
    assert_eq!(doc.mask(), original.mask());
    assert_ne!(doc.layers()[1].raster(), original.layers()[1].raster());
    for p in doc.layers()[1]
        .raster()
        .pixels()
        .chunks_exact(4)
        .filter(|p| p[3] > 0)
    {
        assert_eq!(&p[..3], [255, 0, 0]);
    }
    let before = doc.clone();
    assert!(doc
        .transform_photo_layer(
            0.0,
            [1.0; 2],
            image_editor::PhotoGeometry {
                radial: f64::NAN,
                ..Default::default()
            }
        )
        .is_err());
    assert_eq!(doc, before);
}
