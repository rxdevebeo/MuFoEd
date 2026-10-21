//! Pixel selection, alpha, clipping, and transactional clipboard acceptance.
use image_editor::{Document, PixelClipboard, Raster};

#[test]
fn copy_rectangle_crops_and_preserves_source_while_soft_mask_multiplies_alpha_once() {
    let mut doc = Document::from_raster(Raster::solid(5, 4, [120, 60, 30, 128]).unwrap()).unwrap();
    doc.select_rectangle([1.0, 1.0], [4.0, 3.0]).unwrap();
    let mut mask = doc.mask().to_vec();
    mask[6] = 128;
    mask[7] = 0;
    doc.set_mask(mask).unwrap();
    doc.set_selection(doc.mask().to_vec()).unwrap();
    let before = doc.clone();
    let clip = doc.copy_selection(false).unwrap();
    assert_eq!(doc, before);
    assert_eq!(clip.origin(), [1, 1]);
    assert_eq!([clip.raster().width(), clip.raster().height()], [3, 2]);
    assert_eq!(clip.raster().pixel(0, 0), Some([120, 60, 30, 64]));
    assert_eq!(clip.raster().pixel(1, 0), Some([0; 4]));
    let mut target = Document::new(5, 4).unwrap();
    target.set_mask(vec![0; 20]).unwrap();
    target.paste_pixels(&clip, [1, 1], false).unwrap();
    assert_eq!(
        target.layers()[0].raster().pixel(1, 1),
        Some([120, 60, 30, 64])
    );
}
#[test]
fn polygon_even_odd_concave_clipping_and_invalid_input_are_transactional() {
    let mut doc = Document::new(6, 6).unwrap();
    doc.select_polygon(&[
        [-2.0, -2.0],
        [4.0, -2.0],
        [4.0, 2.0],
        [2.0, 2.0],
        [2.0, 5.0],
        [-2.0, 5.0],
    ])
    .unwrap();
    assert_eq!(
        doc.mask(),
        &[
            255, 255, 255, 255, 0, 0, 255, 255, 255, 255, 0, 0, 255, 255, 0, 0, 0, 0, 255, 255, 0,
            0, 0, 0, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]
    );
    assert_eq!(doc.mask()[3 * 6 + 3], 0);
    assert_eq!(doc.mask()[4 * 6 + 1], 255);
    let before = doc.clone();
    assert!(doc.select_polygon(&[[f32::NAN, 0.0]; 3]).is_err());
    assert_eq!(doc, before);
    assert!(doc.select_polygon(&[[1.0, 1.0]; 2]).is_err());
    assert_eq!(doc, before);
    doc.select_polygon(&[[0.0, 0.0], [4.0, 4.0], [0.0, 4.0], [4.0, 0.0]])
        .unwrap();
    assert!(doc.mask().contains(&255)); // Self-intersection is valid, even-odd fill.
}
#[test]
fn current_layer_paste_composites_and_clips_negative_offsets_without_mutating_selection() {
    let mut doc = Document::from_raster(Raster::solid(3, 2, [0, 0, 200, 255]).unwrap()).unwrap();
    doc.select_rectangle([2.0, 1.0], [3.0, 2.0]).unwrap();
    let mask = doc.mask().to_vec();
    let clip = PixelClipboard::from_raster(Raster::solid(2, 2, [200, 0, 0, 128]).unwrap());
    doc.paste_pixels(&clip, [-1, 0], false).unwrap();
    assert_eq!(doc.layers().len(), 1);
    assert_eq!(
        doc.layers()[0].raster().pixel(0, 0),
        Some([100, 0, 100, 255])
    );
    assert_eq!(doc.layers()[0].raster().pixel(1, 0), Some([0, 0, 200, 255]));
    assert_eq!(doc.mask(), mask);
    let before = doc.clone();
    assert!(doc.paste_pixels(&clip, [i32::MAX, i32::MIN], true).is_err());
    assert_eq!(doc, before);
}
#[test]
fn new_layer_paste_respects_limits_and_current_layer_lock_without_partial_changes() {
    let mut doc = Document::new(3, 3).unwrap();
    doc.set_layer_locked(true);
    let clip = PixelClipboard::from_raster(Raster::solid(1, 1, [9, 8, 7, 255]).unwrap());
    let before = doc.clone();
    assert!(doc.paste_pixels(&clip, [1, 1], false).is_err());
    assert_eq!(doc, before);
    doc.paste_pixels(&clip, [1, 1], true).unwrap();
    assert_eq!(doc.active(), 1);
    assert!(!doc.layers()[1].locked());
    assert_eq!(doc.layers()[1].raster().pixel(1, 1), Some([9, 8, 7, 255]));
    assert_eq!(doc.layers()[1].raster().pixel(0, 0), Some([0; 4]));
    while doc.layers().len() < 16 {
        doc.add_layer("test".into(), Raster::solid(3, 3, [0; 4]).unwrap())
            .unwrap();
    }
    let before = doc.clone();
    assert!(doc.paste_pixels(&clip, [0, 0], true).is_err());
    assert_eq!(doc, before);
}
#[test]
fn unselected_copy_uses_whole_active_layer_or_visible_composite() {
    let mut doc = Document::from_raster(Raster::solid(2, 2, [200, 0, 0, 255]).unwrap()).unwrap();
    doc.add_layer("top".into(), Raster::solid(2, 2, [0, 0, 200, 128]).unwrap())
        .unwrap();
    let clip = doc.copy_selection(false).unwrap();
    assert_eq!(clip.origin(), [0, 0]);
    assert_eq!(clip.raster().pixel(0, 0), Some([0, 0, 200, 128]));
    assert_eq!(doc.copy_selection(true).unwrap().raster(), &doc.flatten());
}

#[test]
fn mask_clipboard_preserves_coverage_and_replaces_mask_without_changing_locked_layers() {
    let mut source = Document::new(4, 3).unwrap();
    source
        .set_mask(vec![0, 0, 0, 0, 0, 64, 0, 255, 0, 128, 0, 0])
        .unwrap();
    let before = source.clone();
    let clip = source.copy_mask().unwrap();
    assert!(clip.is_mask());
    assert_eq!(clip.origin(), [1, 1]);
    assert_eq!(clip.raster().pixel(0, 0), Some([64, 64, 64, 255]));
    assert_eq!(clip.raster().pixel(1, 0), Some([0, 0, 0, 255]));
    assert_eq!(source, before);
    let mut target = Document::from_raster(Raster::solid(4, 3, [8, 16, 32, 255]).unwrap()).unwrap();
    target.set_layer_locked(true);
    target.set_mask(vec![255; 12]).unwrap();
    let layers = target.layers().to_vec();
    target.paste_pixels(&clip, [-1, 0], true).unwrap();
    assert_eq!(target.layers(), layers);
    assert_eq!(target.mask(), &[0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert!(!PixelClipboard::from_raster(clip.raster().clone()).is_mask());
    target.clear_mask();
    let before = target.clone();
    assert!(target.copy_mask().is_err());
    assert_eq!(target, before);
}

#[test]
fn mask_edit_rejects_structural_changes_and_errors_without_partial_mutation() {
    let mut doc = Document::new(8, 8).unwrap();
    doc.select_rectangle([1.0, 1.0], [4.0, 4.0]).unwrap();
    let before = doc.clone();
    assert!(doc.edit_mask(|d| d.crop(0, 0, 4, 4)).is_err());
    assert_eq!(doc, before);
    assert!(doc
        .edit_mask(|d| {
            d.clear_canvas()?;
            Err(image_editor::EditError::Invalid("test failure".into()))
        })
        .is_err());
    assert_eq!(doc, before);
}
