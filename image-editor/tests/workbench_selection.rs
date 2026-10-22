//! Independent manual selection, mask preservation, serialization and geometry acceptance.
use image_editor::{BrushStyle, Document, Raster, SelectionOp, SelectionRefine, Tool};
#[test]
fn selection_is_independent_combines_soft_coverage_and_survives_psd_and_geometry() {
    let mut d = Document::from_raster(Raster::solid(8, 8, [12, 34, 56, 255]).unwrap()).unwrap();
    d.set_mask(vec![77; 64]).unwrap();
    let layers = d.layers().to_vec();
    d.select_area_rectangle([1.0, 1.0], [4.0, 4.0], SelectionOp::Replace)
        .unwrap();
    d.select_area_rectangle([3.0, 3.0], [6.0, 6.0], SelectionOp::Add)
        .unwrap();
    assert_eq!(d.selection().iter().filter(|v| **v > 0).count(), 17);
    d.select_area_rectangle([0.0, 0.0], [3.0, 8.0], SelectionOp::Subtract)
        .unwrap();
    assert_eq!(d.selection()[9], 0);
    assert_eq!(d.selection()[4 * 8 + 4], 255);
    d.refine_selection(SelectionRefine::Feather(1.0)).unwrap();
    assert!(d.selection().iter().any(|v| *v > 0 && *v < 255));
    assert_eq!(d.mask(), &[77; 64]);
    assert_eq!(d.layers(), layers);
    assert_eq!(Document::from_psd(&d.to_psd().unwrap()).unwrap(), d);
    assert_eq!(Document::from_json(&d.to_json().unwrap()).unwrap(), d);
    d.resize(4, 4).unwrap();
    assert_eq!(d.selection().len(), 16);
    assert_eq!(d.mask(), &[77; 16]);
    d.rotate90();
    d.crop(0, 0, 2, 3).unwrap();
    assert_eq!(d.selection().len(), 6);
    d.rotate(20.0, true).unwrap();
    assert_eq!(d.selection().len(), d.mask().len());
}
#[test]
fn grow_shrink_invert_delete_and_paint_respect_coverage_without_touching_mask() {
    let mut d = Document::from_raster(Raster::solid(7, 7, [90, 80, 70, 255]).unwrap()).unwrap();
    d.set_mask(vec![42; 49]).unwrap();
    d.select_area_rectangle([3.0, 3.0], [4.0, 4.0], SelectionOp::Replace)
        .unwrap();
    d.refine_selection(SelectionRefine::Grow(1)).unwrap();
    assert_eq!(d.selection().iter().filter(|v| **v > 0).count(), 9);
    d.refine_selection(SelectionRefine::Shrink(1)).unwrap();
    assert_eq!(d.selection().iter().filter(|v| **v > 0).count(), 1);
    let before = d.clone();
    assert!(d.refine_selection(SelectionRefine::Grow(129)).is_err());
    assert_eq!(d, before);
    d.stroke_with_style(
        [3.5, 3.5],
        [3.5, 3.5],
        6.0,
        [255, 0, 0, 255],
        Tool::Brush,
        BrushStyle::default(),
    )
    .unwrap();
    assert_eq!(d.layers()[0].raster().pixel(0, 0), Some([90, 80, 70, 255]));
    assert_eq!(d.layers()[0].raster().pixel(3, 3), Some([255, 0, 0, 255]));
    d.delete_area(false).unwrap();
    assert_eq!(d.layers()[0].raster().pixel(3, 3).unwrap()[3], 0);
    assert_eq!(d.mask(), &[42; 49]);
    d.refine_selection(SelectionRefine::Invert).unwrap();
    assert_eq!(d.selection().iter().filter(|v| **v > 0).count(), 48);
    d.set_layer_locked(true);
    let before = d.clone();
    assert!(d.delete_area(false).is_err());
    assert_eq!(d, before);
    d.delete_area(true).unwrap();
    assert_eq!(d.layers(), before.layers());
    assert_eq!(d.mask()[0], 0);
    assert_eq!(d.mask()[24], 42);
}
#[test]
fn manual_selection_does_not_clip_paste_and_mask_transfer_is_explicit() {
    let mut d = Document::from_raster(Raster::solid(4, 4, [11, 22, 33, 255]).unwrap()).unwrap();
    d.set_mask(vec![77; 16]).unwrap();
    d.select_area_rectangle([0.0, 0.0], [2.0, 2.0], SelectionOp::Replace)
        .unwrap();
    let clip = d.copy_selection(false).unwrap();
    assert_eq!(clip.raster().width(), 2);
    d.paste_pixels(&clip, [2, 2], true).unwrap();
    assert_eq!(d.layers()[1].raster().pixel(3, 3), Some([11, 22, 33, 255]));
    assert_eq!(d.mask(), &[77; 16]);
    d.selection_to_mask().unwrap();
    assert_eq!(d.mask(), d.selection());
    d.clear_selection();
    assert!(d.selection().is_empty());
    assert_eq!(d.mask()[0], 255);
    d.mask_to_selection().unwrap();
    assert_eq!(d.mask(), d.selection());
}

#[test]
fn selection_outside_a_crop_becomes_deselected_instead_of_invisibly_blocking_paint() {
    let mut doc = Document::new(8, 8).unwrap();
    doc.select_area_rectangle([6.0, 6.0], [8.0, 8.0], SelectionOp::Replace)
        .unwrap();
    doc.crop(0, 0, 3, 3).unwrap();
    assert!(doc.selection().is_empty());
    doc.stroke_with_style(
        [1.5, 1.5],
        [1.5, 1.5],
        1.0,
        [255; 4],
        Tool::Brush,
        BrushStyle::default(),
    )
    .unwrap();
    assert_eq!(doc.layers()[0].raster().pixel(1, 1), Some([255; 4]));
}

#[test]
fn compact_selection_encoding_accepts_legacy_arrays_and_rejects_invalid_hex() {
    let mut doc = Document::new(2, 2).unwrap();
    doc.set_selection(vec![0, 64, 128, 255]).unwrap();
    let mut json: serde_json::Value = serde_json::from_slice(&doc.to_json().unwrap()).unwrap();
    assert_eq!(json["selection"], "004080ff");
    json["selection"] = serde_json::json!([0, 64, 128, 255]);
    assert_eq!(
        Document::from_json(&serde_json::to_vec(&json).unwrap()).unwrap(),
        doc
    );
    for invalid in ["z04080ff", "004080f", "0040"] {
        json["selection"] = serde_json::json!(invalid);
        assert!(Document::from_json(&serde_json::to_vec(&json).unwrap()).is_err());
    }
}
