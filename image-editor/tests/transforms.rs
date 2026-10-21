//! Geometric behavior, mask alignment, alpha quality, and atomic rejection.
use image_editor::{CanvasMode, Document, History, Raster};
fn sample_doc() -> Document {
    let mut doc = Document::from_raster(
        Raster::new(
            2,
            3,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255, 0, 255, 255, 255,
                255, 0, 255, 255,
            ],
        )
        .unwrap(),
    )
    .unwrap();
    doc.set_mask(vec![11, 22, 33, 44, 55, 66]).unwrap();
    doc
}
#[test]
fn right_angle_rotations_are_lossless_for_layers_and_mask() {
    let mut doc = sample_doc();
    let original = doc.clone();
    doc.rotate(90.0, true).unwrap();
    assert_eq!((doc.width(), doc.height()), (3, 2));
    assert_eq!(doc.mask(), &[55, 33, 11, 66, 44, 22]);
    assert_eq!(&doc.layers()[0].raster().pixels()[..4], &[0, 255, 255, 255]);
    doc.rotate(-90.0, true).unwrap();
    assert_eq!(doc, original);
}
#[test]
fn anchored_canvas_bounds_pad_and_crop_without_resampling() {
    let mut doc = sample_doc();
    let original = doc.clone();
    doc.set_layer_locked(true);
    doc.resize_canvas(4, 5, CanvasMode::Bounds, [2, 2]).unwrap();
    assert_eq!(doc.mask()[10..12], [11, 22]);
    assert!(doc.mask()[..10].iter().all(|&v| v == 0));
    assert_eq!(
        &doc.layers()[0].raster().pixels()[40..44],
        &[255, 0, 0, 255]
    );
    assert!(doc.layers()[0].locked());
    doc.resize_canvas(2, 3, CanvasMode::Bounds, [2, 2]).unwrap();
    doc.set_layer_locked(false);
    assert_eq!(doc, original);
}
#[test]
fn fit_pads_fill_crops_and_scale_fills_target() {
    let mut doc = Document::from_raster(Raster::solid(4, 2, [70, 120, 200, 255]).unwrap()).unwrap();
    doc.set_mask(vec![255; 8]).unwrap();
    let original = doc.clone();
    doc.resize_canvas(8, 8, CanvasMode::Fit, [1, 1]).unwrap();
    assert!(doc.mask()[..16].iter().all(|&v| v == 0));
    assert!(doc.mask()[16..48].iter().all(|&v| v == 255));
    assert!(doc.mask()[48..].iter().all(|&v| v == 0));
    for mode in [CanvasMode::Fill, CanvasMode::Scale] {
        let mut changed = original.clone();
        changed.resize_canvas(8, 8, mode, [1, 1]).unwrap();
        assert!(changed.mask().iter().all(|&v| v == 255));
        assert!(changed.layers()[0]
            .raster()
            .pixels()
            .chunks_exact(4)
            .all(|p| p == [70, 120, 200, 255]));
    }
}
#[test]
fn arbitrary_rotation_preserves_color_at_transparent_edges_and_supports_undo() {
    let mut doc = Document::from_raster(Raster::solid(8, 4, [220, 70, 30, 255]).unwrap()).unwrap();
    doc.add_layer(
        "Top".into(),
        Raster::solid(8, 4, [220, 70, 30, 128]).unwrap(),
    )
    .unwrap();
    doc.set_mask(vec![255; 32]).unwrap();
    let original = doc.clone();
    let mut history = History::default();
    history.checkpoint(&doc);
    doc.rotate(37.5, true).unwrap();
    assert_eq!((doc.width(), doc.height()), (9, 9));
    assert_eq!(doc.mask().len(), 81);
    assert!(doc.mask().contains(&0) && doc.mask().contains(&255));
    for layer in doc.layers() {
        let pixels = layer.raster().pixels();
        assert!(pixels.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 128));
        assert!(pixels
            .chunks_exact(4)
            .filter(|p| p[3] > 0)
            .all(|p| p[..3] == [220, 70, 30]));
    }
    let rotated = doc.clone();
    assert!(history.undo(&mut doc));
    assert_eq!(doc, original);
    assert!(history.redo(&mut doc));
    assert_eq!(doc, rotated);
    doc = original;
    doc.rotate(37.5, false).unwrap();
    assert_eq!((doc.width(), doc.height()), (8, 4));
}
#[test]
fn invalid_geometry_is_rejected_atomically_before_allocation() {
    let mut doc = sample_doc();
    let original = doc.clone();
    for angle in [f64::NAN, f64::INFINITY] {
        assert!(doc.rotate(angle, true).is_err());
        assert_eq!(doc, original);
    }
    for (w, h, anchor) in [(0, 5, [1, 1]), (u32::MAX, 2, [1, 1]), (2, 3, [3, 1])] {
        assert!(doc.resize_canvas(w, h, CanvasMode::Bounds, anchor).is_err());
        assert_eq!(doc, original);
    }
    let mut large = Document::new(4096, 4096).unwrap();
    assert!(large.rotate(45.0, true).is_err());
    assert_eq!((large.width(), large.height()), (4096, 4096));
}

#[test]
fn canvas_bounds_preserve_invisible_rgb_and_reject_layer_memory_overflow() {
    let mut doc = Document::from_raster(Raster::solid(2, 2, [90, 70, 50, 0]).unwrap()).unwrap();
    let original = doc.clone();
    doc.resize_canvas(4, 4, CanvasMode::Bounds, [1, 1]).unwrap();
    assert_eq!(&doc.layers()[0].raster().pixels()[20..24], &[90, 70, 50, 0]);
    doc.resize_canvas(2, 2, CanvasMode::Bounds, [1, 1]).unwrap();
    assert_eq!(doc, original);
    doc.add_layer("Second".into(), Raster::solid(2, 2, [0; 4]).unwrap())
        .unwrap();
    let original = doc.clone();
    assert!(doc
        .resize_canvas(4096, 4096, CanvasMode::Scale, [1, 1])
        .is_err());
    assert_eq!(doc, original);
}
