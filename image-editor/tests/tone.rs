//! Levels/curves acceptance independent of UI drawing.
use image_editor::{
    Curves, Document, Histogram, LevelChannel, Levels, Raster, ToneAdjustment, ToneCurve,
};
fn samples() -> Raster {
    Raster::new(
        4,
        1,
        vec![
            0, 64, 255, 255, 32, 128, 224, 128, 255, 127, 1, 0, 80, 100, 120, 255,
        ],
    )
    .unwrap()
}
#[test]
fn neutral_is_byte_exact_even_for_hidden_rgb_and_fractional_selection() {
    let r = samples();
    for a in [
        ToneAdjustment::Levels(Levels::default()),
        ToneAdjustment::Curves(Curves::default()),
    ] {
        assert_eq!(a.apply(&r, &[]).unwrap(), r);
        assert_eq!(a.apply(&r, &[0, 64, 128, 255]).unwrap(), r);
    }
}
#[test]
fn levels_endpoints_gamma_master_and_channel_order_are_defined() {
    let mut l = Levels::default();
    l.channels[0] = LevelChannel {
        input: [32, 224],
        gamma: 1.0,
        output: [10, 210],
    };
    let lut = ToneAdjustment::Levels(l.clone()).lookup_tables().unwrap();
    assert_eq!(lut[0][0], 10);
    assert_eq!(lut[0][32], 10);
    assert_eq!(lut[0][128], 110);
    assert_eq!(lut[0][224], 210);
    assert_eq!(lut[0][255], 210);
    l.channels[1].output = [20, 120];
    let lut = ToneAdjustment::Levels(l).lookup_tables().unwrap();
    assert_eq!(lut[0][128], 63);
    assert_eq!(lut[1][128], 110);
    let mut l = Levels::default();
    l.channels[0].gamma = 2.0;
    let lut = ToneAdjustment::Levels(l).lookup_tables().unwrap();
    assert_eq!(lut[0][64], 128);
}
#[test]
fn smooth_curves_hit_knots_preserve_segment_bounds_and_support_inverse() {
    let c = ToneCurve {
        points: vec![
            [0, 0],
            [40, 80],
            [80, 90],
            [128, 30],
            [200, 210],
            [255, 255],
        ],
    };
    let lut = c.lookup().unwrap();
    for p in &c.points {
        assert_eq!(lut[p[0] as usize], p[1]);
    }
    for w in c.points.windows(2) {
        for &y in &lut[w[0][0] as usize..=w[1][0] as usize] {
            assert!((w[0][1].min(w[1][1])..=w[0][1].max(w[1][1])).contains(&y));
        }
    }
    let inverse = ToneCurve {
        points: vec![[0, 255], [255, 0]],
    }
    .lookup()
    .unwrap();
    for (x, &y) in inverse.iter().enumerate() {
        assert_eq!(usize::from(y), 255 - x);
    }
    let mut curves = Curves::default();
    curves.channels[0].points = vec![[0, 255], [255, 0]];
    curves.channels[3].points = vec![[0, 255], [255, 0]];
    let lut = ToneAdjustment::Curves(curves).lookup_tables().unwrap();
    assert_eq!(lut[0][40], 215);
    assert_eq!(lut[2][40], 40);
}
#[test]
fn correction_preserves_alpha_hidden_rgb_soft_selection_and_other_layers() {
    let mut d = Document::from_raster(samples()).unwrap();
    d.add_layer("target".into(), samples()).unwrap();
    d.set_selection(vec![0, 128, 255, 255]).unwrap();
    d.set_mask(vec![91; 4]).unwrap();
    let before = d.clone();
    let mut l = Levels::default();
    l.channels[0].output = [200, 200];
    let a = ToneAdjustment::Levels(l);
    d.correct_tone(&a).unwrap();
    let r = d.layers()[1].raster();
    assert_eq!(r.pixel(0, 0), before.layers()[1].raster().pixel(0, 0));
    assert_eq!(r.pixel(1, 0), Some([116, 164, 212, 128]));
    assert_eq!(r.pixel(2, 0), Some([255, 127, 1, 0]));
    assert_eq!(r.pixel(3, 0), Some([200, 200, 200, 255]));
    assert_eq!(d.layers()[0], before.layers()[0]);
    assert_eq!(d.mask(), before.mask());
    assert_eq!(d.selection(), before.selection());
    assert_eq!(Document::from_psd(&d.to_psd().unwrap()).unwrap(), d);
    d.set_layer_locked(true);
    let locked = d.clone();
    assert!(d.correct_tone(&a).is_err());
    assert_eq!(d, locked);
}
#[test]
fn histogram_weights_alpha_selection_and_excludes_hidden_rgb() {
    let r = Raster::new(3, 1, vec![255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0]).unwrap();
    let h = Histogram::from_raster(&r, &[128, 255, 255]).unwrap();
    assert_eq!(h.bins[1][255], 32640);
    assert_eq!(h.bins[2][255], 32640);
    assert_eq!(h.bins[3][255], 0);
    assert_eq!(h.bins[0][54], 32640);
    assert_eq!(h.bins[0][182], 32640);
    assert_eq!(h.bins[0].iter().sum::<u64>(), 65280);
    assert!(Histogram::from_raster(&r, &[255]).is_err());
}
#[test]
fn malformed_parameters_fail_before_any_document_change() {
    let mut invalid = Vec::new();
    for gamma in [f32::NAN, f32::INFINITY, 0.0, 11.0] {
        let mut l = Levels::default();
        l.channels[2].gamma = gamma;
        invalid.push(ToneAdjustment::Levels(l));
    }
    let mut l = Levels::default();
    l.channels[0].input = [128, 128];
    invalid.push(ToneAdjustment::Levels(l));
    let mut l = Levels::default();
    l.channels[1].output = [255, 0];
    invalid.push(ToneAdjustment::Levels(l));
    for points in [
        vec![],
        vec![[0, 0]],
        vec![[1, 0], [255, 255]],
        vec![[0, 0], [128, 100], [128, 120], [255, 255]],
        vec![[0, 0], [255, 255], [128, 100]],
    ] {
        let mut c = Curves::default();
        c.channels[0].points = points;
        invalid.push(ToneAdjustment::Curves(c));
    }
    let mut d = Document::from_raster(samples()).unwrap();
    let before = d.clone();
    for a in invalid {
        assert!(d.correct_tone(&a).is_err());
        assert_eq!(d, before);
    }
    assert!(ToneAdjustment::Levels(Levels::default())
        .apply(&samples(), &[0])
        .is_err());
}
#[test]
fn mask_levels_change_only_selection_even_with_a_locked_raster() {
    let mut d = Document::from_raster(samples()).unwrap();
    d.set_layer_locked(true);
    d.set_mask(vec![20, 80, 160, 240]).unwrap();
    d.set_selection(vec![0, 128, 255, 255]).unwrap();
    let before = d.clone();
    let mut l = Levels::default();
    l.channels[0].output = [200, 200];
    let a = ToneAdjustment::Levels(l);
    let preview = a.apply(&d.mask_raster(), d.selection()).unwrap();
    d.edit_mask(|m| m.correct_tone(&a)).unwrap();
    assert_eq!(d.mask(), [20, 140, 200, 200]);
    assert_eq!(d.layers(), before.layers());
    assert_eq!(d.mask_raster(), preview);
}

#[test]
fn composite_preview_matches_export_with_opacity_alpha_and_hidden_layers() {
    let mut d = Document::from_raster(Raster::solid(1, 1, [0, 100, 200, 255]).unwrap()).unwrap();
    d.add_layer(
        "toned".into(),
        Raster::solid(1, 1, [200, 50, 0, 128]).unwrap(),
    )
    .unwrap();
    d.set_layer("toned".into(), true, 128);
    let mut l = Levels::default();
    l.channels[0].output = [100, 100];
    let a = ToneAdjustment::Levels(l);
    let before = d.clone();
    let preview = d.preview_tone(&a).unwrap();
    assert_eq!(d, before);
    assert_eq!(preview.pixel(0, 0), Some([25, 100, 175, 255]));
    d.correct_tone(&a).unwrap();
    assert_eq!(d.flatten(), preview);
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("tone.png");
    d.flatten().save_png(&file).unwrap();
    assert_eq!(Raster::open(&file).unwrap(), preview);
    d.set_layer("hidden".into(), false, 128);
    assert_eq!(d.preview_tone(&a).unwrap(), d.flatten());
}
