//! E02 acceptance: color selection and snapshot-based retouch with alpha/lock/mask.
use image_editor::{BrushStyle, Document, Raster, RetouchMode, SelectionOp};
fn raster(width: u32, pixels: &[[u8; 4]]) -> Raster {
    Raster::new(width, 1, pixels.iter().flatten().copied().collect()).unwrap()
}
#[test]
fn wand_global_tolerance_combination_transparency_and_validation() {
    let source = raster(
        6,
        &[
            [255, 0, 0, 255],
            [250, 0, 0, 255],
            [0, 0, 255, 255],
            [255, 0, 0, 255],
            [255, 90, 70, 0],
            [0, 10, 240, 0],
        ],
    );
    let mut d = Document::from_raster(source.clone()).unwrap();
    d.set_mask(vec![77; 6]).unwrap();
    d.set_layer_locked(true);
    let layers = d.layers().to_vec();
    d.select_by_color(&source, [0, 0], 0, true, SelectionOp::Replace)
        .unwrap();
    assert_eq!(d.selection(), [255, 0, 0, 0, 0, 0]);
    d.select_by_color(&source, [0, 0], 5, true, SelectionOp::Replace)
        .unwrap();
    assert_eq!(d.selection(), [255, 255, 0, 0, 0, 0]);
    d.select_by_color(&source, [0, 0], 5, false, SelectionOp::Replace)
        .unwrap();
    assert_eq!(d.selection(), [255, 255, 0, 255, 0, 0]);
    d.select_by_color(&source, [4, 0], 0, true, SelectionOp::Add)
        .unwrap();
    assert_eq!(d.selection(), [255, 255, 0, 255, 255, 255]);
    d.select_by_color(&source, [0, 0], 0, false, SelectionOp::Subtract)
        .unwrap();
    assert_eq!(d.selection(), [0, 255, 0, 0, 255, 255]);
    d.select_by_color(&source, [5, 0], 0, false, SelectionOp::Intersect)
        .unwrap();
    assert_eq!(d.selection(), [0, 0, 0, 0, 255, 255]);
    let before = d.clone();
    assert!(d
        .select_by_color(&source, [6, 0], 255, false, SelectionOp::Replace)
        .is_err());
    assert_eq!(d, before);
    assert_eq!(d.mask(), [77; 6]);
    assert_eq!(d.layers(), layers);
}
#[test]
fn clone_preserves_alpha_uses_snapshot_and_skips_outside_source() {
    let mut d = Document::from_raster(raster(
        5,
        &[
            [240, 0, 0, 128],
            [0, 180, 0, 255],
            [0, 0, 255, 255],
            [0; 4],
            [0; 4],
        ],
    ))
    .unwrap();
    let brush = d
        .begin_retouch(
            [0.5, 0.5],
            [1.5, 0.5],
            0.5,
            RetouchMode::Clone,
            BrushStyle::default(),
        )
        .unwrap();
    d.retouch_segment(&brush, [1.5, 0.5], [3.5, 0.5]).unwrap();
    assert_eq!(
        d.layers()[0].raster().pixel(2, 0),
        Some([0, 180, 0, 255]),
        "immutable original source"
    );
    assert_eq!(d.layers()[0].raster().pixel(3, 0), Some([0, 0, 255, 255]));
    assert_eq!(d.layers()[0].raster().pixel(1, 0), Some([120, 90, 0, 255]));
    let before = d.clone();
    d.retouch_segment(&brush, [0.5, 0.5], [0.5, 0.5]).unwrap();
    assert_eq!(d, before);
}
#[test]
fn clone_selection_opacity_ai_clip_locks_and_invalid_are_atomic() {
    let mut d = Document::from_raster(raster(
        3,
        &[[200, 0, 0, 255], [0, 0, 0, 255], [0, 0, 0, 255]],
    ))
    .unwrap();
    d.set_selection(vec![0, 128, 255]).unwrap();
    d.set_mask(vec![255, 255, 0]).unwrap();
    let style = BrushStyle {
        opacity: 0.5,
        respect_mask: true,
        ..Default::default()
    };
    let brush = d
        .begin_retouch([0.5, 0.5], [1.5, 0.5], 0.5, RetouchMode::Clone, style)
        .unwrap();
    d.retouch_segment(&brush, [1.5, 0.5], [2.5, 0.5]).unwrap();
    assert_eq!(d.layers()[0].raster().pixel(1, 0), Some([50, 0, 0, 255]));
    assert_eq!(d.layers()[0].raster().pixel(2, 0), Some([0, 0, 0, 255]));
    d.set_layer_locked(true);
    let before = d.clone();
    assert!(d.retouch_segment(&brush, [1.5, 0.5], [1.5, 0.5]).is_err());
    assert!(d
        .begin_retouch([0.5, 0.5], [1.5, 0.5], 0.5, RetouchMode::Clone, style)
        .is_err());
    assert_eq!(d, before);
    d.set_layer_locked(false);
    let before = d.clone();
    assert!(d
        .retouch_segment(&brush, [f32::NAN, 0.0], [1.5, 0.5])
        .is_err());
    assert!(d
        .begin_retouch([-1.0, 0.5], [1.5, 0.5], 0.5, RetouchMode::Clone, style)
        .is_err());
    assert!(d
        .begin_retouch([0.5, 0.5], [1.5, 0.5], 0.0, RetouchMode::Clone, style)
        .is_err());
    assert_eq!(d, before);
}
#[test]
fn heal_matches_local_tone_while_retaining_source_texture() {
    let mut pixels = vec![100; 9 * 3 * 4];
    for p in pixels.chunks_exact_mut(4) {
        p[3] = 255;
    }
    for y in 0..3 {
        for x in 5..9 {
            let i = (y * 9 + x) * 4;
            pixels[i..i + 3].fill(180);
        }
    }
    let i = (9 + 1) * 4;
    pixels[i..i + 3].fill(120);
    let mut d = Document::from_raster(Raster::new(9, 3, pixels).unwrap()).unwrap();
    let b = d
        .begin_retouch(
            [1.5, 1.5],
            [6.5, 1.5],
            1.1,
            RetouchMode::Heal,
            BrushStyle::default(),
        )
        .unwrap();
    d.retouch_segment(&b, [6.5, 1.5], [6.5, 1.5]).unwrap();
    let center = d.layers()[0].raster().pixel(6, 1).unwrap();
    let neighbor = d.layers()[0].raster().pixel(5, 1).unwrap();
    assert!(
        center[0] > 190 && center[0] < 205,
        "texture adapted to destination: {center:?}"
    );
    assert!(neighbor[0] > 170 && neighbor[0] < 181);
    assert_eq!(center[3], 255);
}
#[test]
fn mask_retouch_and_color_selection_leave_locked_raster_intact() {
    let mut d = Document::from_raster(Raster::solid(4, 1, [10, 20, 30, 255]).unwrap()).unwrap();
    d.set_layer_locked(true);
    d.set_mask(vec![200, 0, 0, 0]).unwrap();
    let layers = d.layers().to_vec();
    let sample = d.mask_raster();
    let b = Document::from_raster(sample.clone())
        .unwrap()
        .begin_retouch(
            [0.5, 0.5],
            [2.5, 0.5],
            0.5,
            RetouchMode::Clone,
            BrushStyle::default(),
        )
        .unwrap();
    d.edit_mask(|m| m.retouch_segment(&b, [2.5, 0.5], [2.5, 0.5]))
        .unwrap();
    assert_eq!(d.mask(), [200, 0, 200, 0]);
    assert_eq!(d.layers(), layers);
    d.select_by_color(&sample, [0, 0], 0, false, SelectionOp::Replace)
        .unwrap();
    assert_eq!(d.selection(), [255, 0, 0, 0]);
    assert_eq!(d.mask(), [200, 0, 200, 0]);
}

#[test]
fn wand_four_neighbors_do_not_wrap_rows_or_cross_diagonals() {
    let r = Raster::new(
        2,
        2,
        vec![
            255, 0, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255, 255, 0, 0, 255,
        ],
    )
    .unwrap();
    let mut d = Document::from_raster(r.clone()).unwrap();
    d.select_by_color(&r, [0, 0], 0, true, SelectionOp::Replace)
        .unwrap();
    assert_eq!(d.selection(), [255, 0, 0, 0]);
    d.select_by_color(&r, [0, 0], 0, false, SelectionOp::Replace)
        .unwrap();
    assert_eq!(d.selection(), [255, 0, 0, 255]);
    d.select_by_color(&r, [1, 0], 0, true, SelectionOp::Replace)
        .unwrap();
    assert_eq!(d.selection(), [0, 255, 0, 0]);
    let before = d.clone();
    assert!(d
        .select_by_color(
            &Raster::solid(1, 1, [0; 4]).unwrap(),
            [0, 0],
            0,
            false,
            SelectionOp::Replace
        )
        .is_err());
    assert_eq!(d, before);
    d.select_by_color(&r, [0, 0], 255, true, SelectionOp::Replace)
        .unwrap();
    assert_eq!(d.selection(), [255; 4]);
}
#[test]
fn soft_clone_has_feathered_edges_and_transparent_source_does_not_erase() {
    let mut pixels = vec![0; 12 * 5 * 4];
    for (i, p) in pixels.chunks_exact_mut(4).enumerate() {
        p.copy_from_slice(if i % 12 < 5 {
            &[200, 200, 200, 255]
        } else {
            &[0, 0, 0, 255]
        });
    }
    let base = Document::from_raster(Raster::new(12, 5, pixels).unwrap()).unwrap();
    let mut soft = base.clone();
    let b = soft
        .begin_retouch(
            [2.5, 2.5],
            [9.5, 2.5],
            2.0,
            RetouchMode::Clone,
            BrushStyle {
                hardness: 0.0,
                ..Default::default()
            },
        )
        .unwrap();
    soft.retouch_segment(&b, [9.5, 2.5], [9.5, 2.5]).unwrap();
    assert_eq!(soft.layers()[0].raster().pixel(9, 2).unwrap()[0], 200);
    assert_eq!(soft.layers()[0].raster().pixel(8, 2).unwrap()[0], 100);
    assert_eq!(soft.layers()[0].raster().pixel(7, 2).unwrap()[0], 0);
    let mut d =
        Document::from_raster(raster(3, &[[255, 128, 100, 0], [0, 0, 255, 255], [0; 4]])).unwrap();
    let before = d.clone();
    let b = d
        .begin_retouch(
            [0.5, 0.5],
            [1.5, 0.5],
            0.5,
            RetouchMode::Clone,
            BrushStyle::default(),
        )
        .unwrap();
    d.retouch_segment(&b, [1.5, 0.5], [1.5, 0.5]).unwrap();
    assert_eq!(d, before);
    // Fractional sample between transparent hidden RGB and opaque blue remains blue.
    let b = d
        .begin_retouch(
            [1.0, 0.5],
            [2.5, 0.5],
            0.5,
            RetouchMode::Clone,
            BrushStyle::default(),
        )
        .unwrap();
    d.retouch_segment(&b, [2.5, 0.5], [2.5, 0.5]).unwrap();
    assert_eq!(d.layers()[0].raster().pixel(2, 0), Some([0, 0, 255, 128]));
}
