//! Regressions from the tool-interaction audit.
use image_editor::{Adjustment, ColorCorrection, Document, Raster};

#[test]
fn corrections_honor_soft_selection_and_preserve_alpha_and_unselected_rgb() {
    let source = Raster::new(
        4,
        1,
        vec![
            40, 80, 120, 200, 40, 80, 120, 200, 40, 80, 120, 200, 9, 11, 13, 0,
        ],
    )
    .unwrap();
    for adjustment in [
        Adjustment::Brightness(60),
        Adjustment::Contrast(25.0),
        Adjustment::Grayscale,
        Adjustment::Blur(1.0),
        Adjustment::Median,
    ] {
        let mut all = Document::from_raster(source.clone()).unwrap();
        all.adjust(adjustment).unwrap();
        let mut selected = Document::from_raster(source.clone()).unwrap();
        selected.set_selection(vec![0, 128, 255, 255]).unwrap();
        selected.adjust(adjustment).unwrap();
        check_selected(
            &source,
            all.layers()[0].raster(),
            selected.layers()[0].raster(),
        );
    }
    let correction = ColorCorrection {
        brightness: 0.25,
        ..ColorCorrection::default()
    };
    let all = correction.apply(&source).unwrap();
    let mut selected = Document::from_raster(source.clone()).unwrap();
    selected.set_selection(vec![0, 128, 255, 255]).unwrap();
    selected.correct_color(correction).unwrap();
    check_selected(&source, &all, selected.layers()[0].raster());
}
fn check_selected(source: &Raster, all: &Raster, selected: &Raster) {
    assert_eq!(selected.pixel(0, 0), source.pixel(0, 0), "unselected pixel");
    assert_eq!(selected.pixel(3, 0), source.pixel(3, 0), "hidden RGB");
    assert_eq!(selected.pixel(2, 0), all.pixel(2, 0), "full coverage");
    let from = source.pixel(1, 0).unwrap();
    let to = all.pixel(1, 0).unwrap();
    let actual = selected.pixel(1, 0).unwrap();
    assert_eq!(actual[3], from[3]);
    for c in 0..3 {
        assert_eq!(
            u32::from(actual[c]),
            (u32::from(from[c]) * 127 + u32::from(to[c]) * 128 + 127) / 255,
            "soft coverage"
        );
    }
}
#[test]
fn mask_corrections_respect_selection_without_mutating_layers() {
    let mut doc = Document::new(4, 1).unwrap();
    doc.set_mask(vec![60; 4]).unwrap();
    doc.set_selection(vec![0, 128, 255, 0]).unwrap();
    let layers = doc.layers().to_vec();
    doc.edit_mask(|d| d.adjust(Adjustment::Brightness(100)))
        .unwrap();
    assert_eq!(doc.mask(), [60, 110, 160, 60]);
    assert_eq!(doc.layers(), layers);
}
