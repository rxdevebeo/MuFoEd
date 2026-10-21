//! Independent psd-tools fixtures, plus raw unsupported-mask preflight.
use image_editor::{Document, LayerBlendMode as Blend, Raster};
#[test]
fn independent_standard_layer_mask_and_blend_fixtures() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd");
    for (case, mode) in [
        ("normal", Blend::Normal),
        ("multiply", Blend::Multiply),
        ("screen", Blend::Screen),
        ("overlay", Blend::Overlay),
        ("disabled", Blend::Normal),
        ("offset", Blend::Normal),
    ] {
        let doc = Document::open_project(root.join(format!("layer-{case}.psd"))).unwrap();
        let layer = &doc.layers()[1];
        assert_eq!(layer.blend_mode(), mode);
        assert_eq!(layer.opacity(), 173);
        assert_eq!(layer.raster().pixel(1, 1), Some([200, 80, 40, 128]));
        let mask = layer.mask().unwrap();
        assert_eq!(mask.enabled(), case != "disabled");
        if case == "offset" {
            assert_eq!(mask.default_color(), 255);
            assert_eq!(
                mask.pixels(),
                [255, 255, 255, 255, 255, 0, 128, 255, 255, 255, 255, 255]
            );
        } else {
            assert_eq!(mask.pixels(), [0, 64, 128, 255].repeat(3));
        }
        let reference = Raster::open(root.join(format!("layer-{case}.png"))).unwrap();
        assert!(
            doc.flatten()
                .pixels()
                .iter()
                .zip(reference.pixels())
                .all(|(&a, &b)| a.abs_diff(b) <= 1),
            "{case}"
        );
        assert_eq!(Document::from_psd(&doc.to_psd().unwrap()).unwrap(), doc);
    }
}
#[test]
fn unsupported_raw_mask_flags_and_unknown_blend_are_rejected() {
    let mut doc = Document::new(4, 3).unwrap();
    doc.add_layer_mask(true).unwrap();
    let bytes = doc.to_psd().unwrap();
    // Standard 20-byte mask record: four i32 bounds, default, flags, padding.
    let record: Vec<u8> = [20u32, 0, 0, 3, 4]
        .into_iter()
        .flat_map(u32::to_be_bytes)
        .chain([255, 0, 0, 0])
        .collect();
    let offset = bytes
        .windows(record.len())
        .position(|v| v == record)
        .unwrap();
    for flags in [4, 8, 16, 32, 128] {
        let mut bad = bytes.clone();
        bad[offset + 21] = flags;
        assert!(Document::from_psd(&bad).is_err(), "flags {flags}");
    }
    let mut bad = bytes;
    let key = bad.windows(8).position(|v| v == b"8BIMnorm").unwrap();
    bad[key + 4..key + 8].copy_from_slice(b"zzzz");
    assert!(Document::from_psd(&bad).is_err());
}
