//! Conventional paste and stroke contracts (U01/U02).
use image_editor::{BrushStyle, Document, MaskTarget, PixelClipboard, Raster, RetouchMode, Tool};
#[test]
fn mask_paste_preserves_outside_holes_soft_alpha_and_layer_lock() {
    let mut source = Document::new(3, 1).unwrap();
    source.set_mask(vec![200, 100, 0]).unwrap();
    source.set_selection(vec![128, 0, 255]).unwrap();
    let clip = source.copy_mask().unwrap();
    assert_eq!(clip.raster().pixel(0, 0), Some([200, 200, 200, 128]));
    assert_eq!(clip.raster().pixel(1, 0), Some([100, 100, 100, 0]));
    let mut doc = Document::new(5, 2).unwrap();
    doc.set_mask(vec![40; 10]).unwrap();
    doc.paste_pixels(&clip, [1, 0], true).unwrap();
    assert_eq!(doc.mask(), [40, 120, 40, 0, 40, 40, 40, 40, 40, 40]);
    doc.add_layer_mask(true).unwrap();
    let layers = doc.layers().to_vec();
    doc.edit_target_mask(MaskTarget::Layer(0), |d| {
        d.paste_pixels(
            &PixelClipboard::from_raster(Raster::solid(1, 1, [90, 90, 90, 128]).unwrap())
                .for_mask(false),
            [1, 1],
            false,
        )
    })
    .unwrap();
    assert_eq!(doc.layers()[0].mask().unwrap().pixels()[0], 255);
    assert_ne!(doc.layers()[0].mask(), layers[0].mask());
    doc.set_layer_locked(true);
    let before = doc.clone();
    assert!(doc
        .edit_target_mask(MaskTarget::Layer(0), |d| d
            .correct_color(image_editor::ColorCorrection::default()))
        .is_err());
    assert_eq!(doc, before);
}
#[test]
fn opacity_cap_is_independent_of_subdivision_and_stroke_crossings() {
    for tool in [Tool::Brush, Tool::Eraser] {
        let source = Raster::solid(
            32,
            16,
            if tool == Tool::Brush {
                [0; 4]
            } else {
                [90, 100, 110, 255]
            },
        )
        .unwrap();
        let style = BrushStyle {
            opacity: 0.4,
            hardness: 0.5,
            respect_mask: true,
        };
        let run = |steps: u16| {
            let mut d = Document::from_raster(source.clone()).unwrap();
            d.set_selection(vec![180; 512]).unwrap();
            d.set_mask(vec![200; 512]).unwrap();
            let mut stroke = d
                .begin_brush_stroke(4.0, [240, 20, 50, 255], tool, style)
                .unwrap();
            for n in 0..steps {
                let a = 4.5 + 20.0 * f32::from(n) / f32::from(steps);
                let b = 4.5 + 20.0 * f32::from(n + 1) / f32::from(steps);
                d.brush_segment(&mut stroke, [a, 8.5], [b, 8.5]).unwrap();
            }
            for _ in 0..10 {
                d.brush_segment(&mut stroke, [24.5, 8.5], [4.5, 8.5])
                    .unwrap();
            }
            d
        };
        assert_eq!(run(1), run(40));
    }
}
#[test]
fn separate_brush_strokes_accumulate_but_a_single_retouch_does_not() {
    let mut d = Document::new(8, 8).unwrap();
    let style = BrushStyle {
        opacity: 0.5,
        ..Default::default()
    };
    for expected in [128, 192] {
        let mut stroke = d
            .begin_brush_stroke(2.0, [255; 4], Tool::Brush, style)
            .unwrap();
        for _ in 0..10 {
            d.brush_segment(&mut stroke, [3.5; 2], [3.5; 2]).unwrap();
        }
        assert_eq!(d.layers()[0].raster().pixel(3, 3).unwrap()[3], expected);
    }
    let mut d = Document::from_raster(
        Raster::new(
            4,
            1,
            vec![240, 20, 20, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255],
        )
        .unwrap(),
    )
    .unwrap();
    let stroke = d
        .begin_retouch([0.5, 0.5], [2.5, 0.5], 0.5, RetouchMode::Clone, style)
        .unwrap();
    d.retouch_segment(&stroke, [2.5, 0.5], [2.5, 0.5]).unwrap();
    let first = d.clone();
    for _ in 0..20 {
        d.retouch_segment(&stroke, [2.5, 0.5], [2.5, 0.5]).unwrap();
    }
    assert_eq!(d, first);
}
