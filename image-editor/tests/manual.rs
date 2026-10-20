//! Manual workbench contracts: alpha, selection, layer identity and persistence.
use image_editor::{Adjustment, BrushStyle, Document, History, Raster, Shape, Tool};

const RED: [u8; 4] = [255, 0, 0, 255];

#[test]
fn locked_pixels_reject_edits_atomically_but_mask_and_copy_remain_available() {
    let mut d = Document::new(8, 8).unwrap();
    d.set_layer_locked(true);
    let original = d.clone();
    assert!(d.stroke([1.0; 2], [5.0; 2], 2.0, RED, Tool::Brush).is_err());
    assert!(d.bucket_fill(0, 0, RED, 0, BrushStyle::default()).is_err());
    assert!(d
        .gradient([0.0; 2], [8.0; 2], RED, [0; 4], BrushStyle::default())
        .is_err());
    assert!(d
        .draw_shape(
            [1.0; 2],
            [5.0; 2],
            Shape::Rectangle,
            1.0,
            true,
            RED,
            BrushStyle::default()
        )
        .is_err());
    assert!(d.translate_layer(1, 0).is_err());
    assert!(d.adjust(Adjustment::Grayscale).is_err());
    assert!(d.remove_layer().is_err());
    assert_eq!(d, original);
    d.select_rectangle([1.0; 2], [4.0; 2]).unwrap();
    assert!(d.mask().contains(&255));
    d.duplicate_layer().unwrap();
    assert!(!d.layers()[d.active()].locked());
    assert!(d.layers()[0].locked());
}

#[test]
fn reorder_preserves_active_identity_in_both_directions() {
    let mut d = Document::new(4, 4).unwrap();
    for name in ["A", "B", "C"] {
        d.add_layer(name.into(), Raster::solid(4, 4, RED).unwrap())
            .unwrap();
    }
    d.select(2).unwrap();
    d.reorder_layer(0, 3).unwrap();
    assert_eq!(d.layers()[d.active()].name(), "B");
    d.reorder_layer(3, 0).unwrap();
    assert_eq!(d.layers()[d.active()].name(), "B");
    d.reorder_layer(2, 0).unwrap();
    assert_eq!(d.active(), 0);
    assert_eq!(d.layers()[0].name(), "B");
    let before = d.clone();
    assert!(d.reorder_layer(20, 0).is_err());
    assert_eq!(d, before);
}

#[test]
fn merge_bakes_opacity_and_undo_restores_both_layers() {
    let mut d = Document::from_raster(Raster::solid(4, 4, [255, 0, 0, 100]).unwrap()).unwrap();
    d.add_layer("Top".into(), Raster::solid(4, 4, [0, 0, 255, 128]).unwrap())
        .unwrap();
    d.set_layer("Top".into(), true, 180);
    let before = d.clone();
    let composite = d.flatten();
    let mut history = History::default();
    history.checkpoint(&d);
    d.merge_down().unwrap();
    assert_eq!(d.flatten(), composite);
    assert_eq!(d.layers().len(), 1);
    assert!(history.undo(&mut d));
    assert_eq!(d, before);
    d.select(0).unwrap();
    d.set_layer_locked(true);
    d.select(1).unwrap();
    let locked = d.clone();
    assert!(d.merge_down().is_err());
    assert_eq!(d, locked);
}

#[test]
fn flood_fill_is_connected_and_does_not_wrap_rows() {
    let mut pixels = [0, 0, 0, 255].repeat(9);
    for i in [2, 3, 4, 5, 6, 7, 8] {
        pixels[i * 4..i * 4 + 4].copy_from_slice(&RED);
    }
    // Black at (0,0),(1,0), white at (0,1): row-end must not connect to next row.
    pixels[3 * 4..3 * 4 + 4].copy_from_slice(&[0, 0, 0, 255]);
    let mut d = Document::from_raster(Raster::new(3, 3, pixels).unwrap()).unwrap();
    d.bucket_fill(1, 0, [0, 255, 0, 255], 0, BrushStyle::default())
        .unwrap();
    assert_eq!(d.flatten().pixel(0, 1), Some([0, 255, 0, 255]));
    assert_eq!(d.flatten().pixel(2, 0), Some(RED));
    let mut d = Document::from_raster(
        Raster::new(2, 2, [RED, [0, 0, 0, 255], [0, 0, 0, 255], RED].concat()).unwrap(),
    )
    .unwrap();
    d.bucket_fill(1, 0, [0, 255, 0, 255], 0, BrushStyle::default())
        .unwrap();
    assert_eq!(d.flatten().pixel(0, 1), Some([0, 0, 0, 255]));
}

#[test]
fn selection_clips_paint_and_move_never_wraps() {
    let mut d = Document::new(8, 8).unwrap();
    d.select_rectangle([2.0; 2], [5.0; 2]).unwrap();
    assert!(d.flatten().pixels().iter().all(|v| *v == 0));
    let style = BrushStyle {
        respect_mask: true,
        ..BrushStyle::default()
    };
    d.bucket_fill(3, 3, RED, 0, style).unwrap();
    assert_eq!(d.flatten().pixel(3, 3), Some(RED));
    assert_eq!(d.flatten().pixel(1, 3), Some([0; 4]));
    d.translate_layer(5, 0).unwrap();
    assert_eq!(d.flatten().pixel(7, 3), Some(RED));
    assert_eq!(d.flatten().pixel(0, 3), Some([0; 4]));
}

#[test]
fn transparent_gradient_has_no_hidden_color_fringe_and_invalid_input_is_atomic() {
    let mut d = Document::new(8, 8).unwrap();
    d.gradient(
        [0.0, 0.0],
        [8.0, 0.0],
        RED,
        [0, 255, 0, 0],
        BrushStyle::default(),
    )
    .unwrap();
    let p = d.flatten().pixel(4, 0).unwrap();
    assert_eq!(&p[..3], &[255, 0, 0]);
    assert!(p[3] > 0 && p[3] < 255);
    let before = d.clone();
    assert!(d
        .gradient([f32::NAN, 0.0], [8.0, 0.0], RED, RED, BrushStyle::default())
        .is_err());
    assert_eq!(d, before);
}

#[test]
fn layer_lock_roundtrips_and_old_projects_default_to_unlocked() {
    let mut d = Document::new(4, 4).unwrap();
    d.set_layer_locked(true);
    assert_eq!(Document::from_json(&d.to_json().unwrap()).unwrap(), d);
    let mut json: serde_json::Value = serde_json::from_slice(&d.to_json().unwrap()).unwrap();
    json["layers"][0].as_object_mut().unwrap().remove("locked");
    let restored = Document::from_json(json.to_string().as_bytes()).unwrap();
    assert!(!restored.layers()[0].locked());
}
