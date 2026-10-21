//! Non-destructive masks, alpha-aware blends, geometry and bounded versioned projects.
use image_editor::{
    BrushStyle, CanvasMode, Document, History, LayerBlendMode as Blend, MaskTarget, Raster, Tool,
};
fn pixel(r: &Raster) -> [u8; 4] {
    r.pixel(0, 0).unwrap()
}
#[test]
fn opaque_blend_reference_values_and_transparent_backdrop_are_correct() {
    for (mode, expected) in [
        (Blend::Normal, [192, 128, 64, 255]),
        (Blend::Multiply, [48, 64, 48, 255]),
        (Blend::Screen, [208, 192, 208, 255]),
        (Blend::Overlay, [96, 128, 161, 255]),
    ] {
        let mut d =
            Document::from_raster(Raster::solid(1, 1, [64, 128, 192, 255]).unwrap()).unwrap();
        d.add_layer(
            "source".into(),
            Raster::solid(1, 1, [192, 128, 64, 255]).unwrap(),
        )
        .unwrap();
        d.set_blend_mode(mode).unwrap();
        assert_eq!(pixel(&d.flatten()), expected);
        let mut d = Document::from_raster(Raster::solid(1, 1, [33, 77, 210, 0]).unwrap()).unwrap();
        d.add_layer(
            "source".into(),
            Raster::solid(1, 1, [192, 128, 64, 128]).unwrap(),
        )
        .unwrap();
        d.set_layer("source".into(), true, 128);
        d.set_layer_mask(vec![128]).unwrap();
        d.set_blend_mode(mode).unwrap();
        assert_eq!(
            pixel(&d.flatten()),
            [192, 128, 64, 32],
            "transparent backdrop must not darken source: {mode:?}"
        );
    }
}
#[test]
fn layer_mask_is_independent_disabled_reversible_and_bakes_only_alpha() {
    let mut d = Document::from_raster(Raster::solid(3, 1, [210, 60, 30, 255]).unwrap()).unwrap();
    d.set_mask(vec![77; 3]).unwrap();
    d.set_selection(vec![128; 3]).unwrap();
    let raw = d.layers()[0].raster().clone();
    d.set_layer_mask(vec![0, 128, 255]).unwrap();
    assert_eq!(d.layers()[0].raster(), &raw);
    assert_eq!(d.flatten().pixel(0, 0), Some([0; 4]));
    assert_eq!(d.flatten().pixel(1, 0), Some([210, 60, 30, 128]));
    let visible = d.flatten();
    d.set_layer_mask_enabled(false).unwrap();
    assert_eq!(d.flatten(), raw);
    assert_eq!(d.layers()[0].mask().unwrap().pixels(), [0, 128, 255]);
    d.set_layer_mask_enabled(true).unwrap();
    d.apply_layer_mask().unwrap();
    assert_eq!(d.flatten(), visible);
    assert!(d.layers()[0].mask().is_none());
    assert_eq!(d.layers()[0].raster().pixel(0, 0), Some([210, 60, 30, 0]));
    assert_eq!(d.mask(), [77; 3]);
    assert_eq!(d.selection(), [128; 3]);
    d.add_layer_mask(false).unwrap();
    d.set_layer_mask_enabled(false).unwrap();
    let before = d.flatten();
    d.apply_layer_mask().unwrap();
    assert_eq!(d.flatten(), before);
}
#[test]
fn shared_pixel_tools_edit_only_chosen_mask_honor_selection_lock_and_undo() {
    let mut d = Document::from_raster(Raster::solid(4, 1, [10, 20, 30, 255]).unwrap()).unwrap();
    d.add_layer_mask(false).unwrap();
    d.set_mask(vec![42; 4]).unwrap();
    d.set_selection(vec![255, 128, 0, 0]).unwrap();
    let before = d.clone();
    let mut history = History::default();
    history.checkpoint(&d);
    d.edit_target_mask(MaskTarget::Layer(0), |m| {
        m.stroke_with_style(
            [0.5, 0.5],
            [3.5, 0.5],
            0.5,
            [255; 4],
            Tool::Brush,
            BrushStyle::default(),
        )
    })
    .unwrap();
    assert_eq!(d.layers()[0].mask().unwrap().pixels(), [255, 128, 0, 0]);
    assert_eq!(d.mask(), before.mask());
    assert_eq!(d.layers()[0].raster(), before.layers()[0].raster());
    assert_eq!(d.selection(), before.selection());
    assert!(history.undo(&mut d));
    assert_eq!(d, before);
    assert!(!history.undo(&mut d));
    assert!(history.redo(&mut d));
    d.set_layer_locked(true);
    let locked = d.clone();
    assert!(d
        .edit_target_mask(MaskTarget::Layer(0), Document::clear_canvas)
        .is_err());
    assert_eq!(d, locked);
    assert!(d.copy_target_mask(MaskTarget::Layer(0)).is_ok());
    d.edit_target_mask(MaskTarget::Inpaint, |m| {
        m.stroke([0.5, 0.5], [0.5, 0.5], 0.5, [255; 4], Tool::Brush)
    })
    .unwrap();
    assert_eq!(d.mask()[0], 255);
    assert_eq!(d.layers(), locked.layers());
}
#[test]
fn layer_mask_context_rejects_structural_mutation_or_invalid_target_atomically() {
    let mut d = Document::new(4, 4).unwrap();
    d.add_layer_mask(true).unwrap();
    let before = d.clone();
    assert!(d
        .edit_target_mask(MaskTarget::Layer(0), |m| m.resize(2, 2))
        .is_err());
    assert_eq!(d, before);
    assert!(d
        .edit_target_mask(MaskTarget::Layer(1), |_| Ok(()))
        .is_err());
    assert_eq!(d, before);
    assert!(d.set_layer_mask(vec![0; 15]).is_err());
    assert_eq!(d, before);
}
#[test]
fn linked_masks_follow_translation_mirror_quarter_turn_crop_and_resize() {
    let mut d = Document::from_raster(Raster::solid(4, 4, [200, 100, 50, 255]).unwrap()).unwrap();
    let mut coverage = vec![0; 16];
    coverage[5] = 255;
    d.set_layer_mask(coverage).unwrap();
    d.set_mask(vec![17; 16]).unwrap();
    d.translate_layer(1, 0).unwrap();
    assert_eq!(d.flatten().pixel(2, 1), Some([200, 100, 50, 255]));
    assert_eq!(d.flatten().pixel(1, 1).unwrap()[3], 0);
    assert_eq!(d.mask(), [17; 16]);
    d.flip_layer(true).unwrap();
    assert_eq!(d.flatten().pixel(1, 1).unwrap()[3], 255);
    d.transform_photo_layer_about(
        90.0,
        [1.0; 2],
        image_editor::PhotoGeometry::default(),
        [2.0, 2.0],
    )
    .unwrap();
    assert_eq!(d.flatten().pixel(2, 1).unwrap()[3], 255);
    d.rotate90();
    assert_eq!(d.flatten().pixel(2, 2).unwrap()[3], 255);
    d.crop(1, 1, 2, 2).unwrap();
    assert_eq!(d.flatten().pixel(1, 1).unwrap()[3], 255);
    assert_eq!(d.layers()[0].mask().unwrap().pixels().len(), 4);
    d.resize(4, 4).unwrap();
    assert_eq!(d.layers()[0].mask().unwrap().pixels()[10], 255);
    assert_eq!(d.layers()[0].mask().unwrap().pixels()[0], 0);
    d.resize_canvas(6, 6, CanvasMode::Bounds, [1, 1]).unwrap();
    assert_eq!(d.layers()[0].mask().unwrap().pixels().len(), 36);
    d.rotate(13.0, true).unwrap();
    assert_eq!(d.layers()[0].mask().unwrap().pixels().len(), d.mask().len());
    assert_eq!(Document::from_json(&d.to_json().unwrap()).unwrap(), d);
}
#[test]
fn linked_white_default_does_not_double_fade_rotated_alpha_and_black_stays_hidden() {
    let mut plain =
        Document::from_raster(Raster::solid(8, 8, [220, 150, 80, 255]).unwrap()).unwrap();
    let mut masked = plain.clone();
    masked.add_layer_mask(true).unwrap();
    plain.transform_layer(37.0, [1.0; 2]).unwrap();
    masked.transform_layer(37.0, [1.0; 2]).unwrap();
    assert_eq!(masked.flatten(), plain.flatten());
    let mut hidden =
        Document::from_raster(Raster::solid(8, 8, [220, 150, 80, 255]).unwrap()).unwrap();
    hidden.add_layer_mask(false).unwrap();
    hidden.transform_layer(37.0, [1.0; 2]).unwrap();
    assert!(hidden.flatten().pixels().chunks_exact(4).all(|p| p[3] == 0));
}
#[test]
fn json_versions_and_psd_roundtrip_mask_defaults_modes_locks_and_planes() {
    for mode in [
        Blend::Normal,
        Blend::Multiply,
        Blend::Screen,
        Blend::Overlay,
    ] {
        let mut d = Document::from_raster(Raster::solid(4, 3, [50, 70, 90, 128]).unwrap()).unwrap();
        d.add_layer_mask(true).unwrap();
        d.set_layer_mask([0, 64, 128, 255].repeat(3)).unwrap();
        d.set_layer_mask_enabled(false).unwrap();
        d.set_blend_mode(mode).unwrap();
        d.set_mask(vec![37; 12]).unwrap();
        d.set_selection(vec![91; 12]).unwrap();
        d.set_layer_locked(true);
        let json = d.to_json().unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&json).unwrap()["version"],
            2
        );
        assert_eq!(Document::from_json(&json).unwrap(), d);
        assert_eq!(Document::from_psd(&d.to_psd().unwrap()).unwrap(), d);
        d.set_layer_locked(false);
        d.remove_layer_mask().unwrap();
        d.set_blend_mode(Blend::Normal).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&d.to_json().unwrap()).unwrap()["version"],
            1
        );
    }
}
#[test]
fn merge_bakes_masks_and_modes_at_bottom_but_rejects_backdrop_dependent_pairs() {
    let mut d = Document::from_raster(Raster::solid(2, 1, [40, 100, 160, 255]).unwrap()).unwrap();
    d.add_layer(
        "top".into(),
        Raster::solid(2, 1, [200, 80, 40, 128]).unwrap(),
    )
    .unwrap();
    d.set_layer_mask(vec![64, 255]).unwrap();
    d.set_blend_mode(Blend::Overlay).unwrap();
    let expected = d.flatten();
    d.merge_down().unwrap();
    assert_eq!(d.flatten(), expected);
    assert!(d.layers()[0].mask().is_none());
    assert_eq!(d.layers()[0].blend_mode(), Blend::Normal);
    d.add_layer("middle".into(), Raster::solid(2, 1, [100; 4]).unwrap())
        .unwrap();
    d.add_layer("top".into(), Raster::solid(2, 1, [100; 4]).unwrap())
        .unwrap();
    d.set_blend_mode(Blend::Screen).unwrap();
    let before = d.clone();
    assert!(d.merge_down().is_err());
    assert_eq!(d, before);
}
#[test]
fn layer_mask_memory_is_counted_for_creation_duplication_and_resizing() {
    let mut d = Document::new(1024, 1024).unwrap();
    for i in 0..12 {
        if i > 0 {
            d.add_layer(
                format!("layer {i}"),
                Raster::solid(1024, 1024, [0; 4]).unwrap(),
            )
            .unwrap();
        }
        d.add_layer_mask(true).unwrap();
    }
    let before = d.clone();
    assert!(d.resize(1536, 1536).is_err());
    assert_eq!(d, before);
    assert!(d
        .resize_canvas(1536, 1536, CanvasMode::Scale, [1, 1])
        .is_err());
    assert_eq!(d, before);
    let mut d = Document::new(1536, 1536).unwrap();
    for i in 1..12 {
        d.add_layer(
            format!("layer {i}"),
            Raster::solid(1536, 1536, [0; 4]).unwrap(),
        )
        .unwrap();
    }
    for i in 0..7 {
        d.select(i).unwrap();
        d.add_layer_mask(true).unwrap();
    }
    d.select(7).unwrap();
    let before = d.clone();
    assert!(d.add_layer_mask(true).is_err());
    assert_eq!(d, before);
    assert!(d.duplicate_layer().is_err());
    assert_eq!(d, before);
}
