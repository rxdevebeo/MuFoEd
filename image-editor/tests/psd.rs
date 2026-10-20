//! PSD project roundtrips and refusal of unsupported/lossy edits.
use ag_psd::psd::{BlendMode, Layer, LayerAdditionalInfo, PixelData, Psd, WriteOptions};
use image_editor::{generation::Request, Document, Raster, Tool};
fn simple(layer: Layer) -> Vec<u8> {
    ag_psd::write_psd(
        &Psd {
            width: 4.0,
            height: 3.0,
            children: Some(vec![layer]),
            ..Default::default()
        },
        &WriteOptions {
            no_background: Some(true),
            ..Default::default()
        },
    )
}
fn layer() -> Layer {
    Layer {
        left: Some(1.0),
        top: Some(1.0),
        right: Some(3.0),
        bottom: Some(2.0),
        image_data: Some(PixelData {
            width: 2,
            height: 1,
            data: [17, 55, 90, 123].repeat(2),
        }),
        additional_info: LayerAdditionalInfo {
            name: Some("External offset layer".into()),
            ..Default::default()
        },
        ..Default::default()
    }
}
#[test]
fn psd_retains_exact_layers_mask_selection_recipe_and_composite() {
    let mut doc = Document::from_raster(Raster::solid(8, 6, [20, 80, 190, 255]).unwrap()).unwrap();
    doc.set_layer("Фон 背景".into(), true, 255);
    doc.add_generated_layer(
        "Vase 🎨".into(),
        Raster::solid(8, 6, [210, 20, 60, 127]).unwrap(),
        Request {
            seed: u64::MAX,
            ..Default::default()
        },
    )
    .unwrap();
    doc.set_layer("Vase 🎨".into(), true, 173);
    doc.stroke([2.0, 2.0], [5.0, 2.0], 1.0, [0; 4], Tool::Mask)
        .unwrap();
    doc.set_layer_locked(true);
    doc.add_layer(
        "Invisible".into(),
        Raster::solid(8, 6, [40, 50, 60, 0]).unwrap(),
    )
    .unwrap();
    doc.set_layer("Invisible".into(), false, 0);
    doc.select(1).unwrap();
    let bytes = doc.to_psd().unwrap();
    assert_eq!(&bytes[..4], b"8BPS");
    let loaded = Document::from_psd(&bytes).unwrap();
    assert_eq!(loaded, doc);
    assert_eq!(loaded.flatten(), doc.flatten());
}
#[test]
fn external_layer_bounds_are_placed_without_changing_source_pixels() {
    let doc = Document::from_psd(&simple(layer())).unwrap();
    let pixels = doc.layers()[0].raster().pixels();
    assert_eq!(&pixels[20..28], &[17, 55, 90, 123, 17, 55, 90, 123]);
    assert!(pixels[..20].iter().all(|v| *v == 0));
    assert_eq!(doc.layers()[0].name(), "External offset layer");
}
#[test]
fn unsupported_layers_and_outside_canvas_pixels_are_refused() {
    let mut l = layer();
    l.blend_mode = Some(BlendMode::Multiply);
    assert!(Document::from_psd(&simple(l))
        .unwrap_err()
        .to_string()
        .contains("Unsupported PSD layer"));
    let mut l = layer();
    l.left = Some(-1.0);
    l.right = Some(1.0);
    assert!(Document::from_psd(&simple(l))
        .unwrap_err()
        .to_string()
        .contains("outside the canvas"));
    let group = Layer {
        children: Some(vec![layer()]),
        ..Default::default()
    };
    assert!(Document::from_psd(&simple(group)).is_err());
}
#[test]
fn malformed_header_dimensions_depth_and_every_truncation_are_errors() {
    let bytes = Document::new(4, 3).unwrap().to_psd().unwrap();
    for n in 0..bytes.len() {
        assert!(
            Document::from_psd(&bytes[..n]).is_err(),
            "accepted truncation {n}"
        );
    }
    for (offset, value) in [(5, 2), (23, 16), (25, 4)] {
        let mut bad = bytes.clone();
        bad[offset] = value;
        assert!(Document::from_psd(&bad).is_err());
    }
    let mut bad = bytes;
    bad[18..22].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(Document::from_psd(&bad).is_err());
}
#[test]
fn save_dispatch_is_atomic_and_legacy_json_still_opens() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.PSD");
    let doc = Document::new(3, 2).unwrap();
    doc.save_project(&path).unwrap();
    assert_eq!(Document::open_project(&path).unwrap(), doc);
    let legacy = dir.path().join("legacy.json");
    doc.save_project(&legacy).unwrap();
    assert_eq!(Document::open_project(legacy).unwrap(), doc);
    let previous = std::fs::read(&path).unwrap();
    let wide = Document::new(30001, 1).unwrap();
    assert!(wide.save_project(&path).is_err());
    assert_eq!(std::fs::read(path).unwrap(), previous);
    assert!(doc.save_project(dir.path().join("mistyped.png")).is_err());
}

#[test]
fn independent_writer_rle_zip_offsets_and_rejections() {
    let doc = Document::from_psd(include_bytes!("fixtures/psd/external-rgb8.psd")).unwrap();
    assert_eq!((doc.width(), doc.height(), doc.layers().len()), (4, 3, 2));
    assert_eq!(doc.layers()[0].name(), "Background");
    assert_eq!(&doc.layers()[0].raster().pixels()[..4], &[10, 20, 30, 255]);
    assert_eq!(doc.layers()[1].opacity(), 173);
    assert_eq!(
        &doc.layers()[1].raster().pixels()[20..28],
        &[100, 150, 200, 128, 100, 150, 200, 128]
    );
    assert!(Document::from_psd(include_bytes!("fixtures/psd/external-multiply.psd")).is_err());
    assert!(Document::from_psd(include_bytes!("fixtures/psd/external-mask.psd")).is_err());
    assert!(
        Document::from_psd(include_bytes!("fixtures/psd/external-rgb16.psd"))
            .unwrap_err()
            .to_string()
            .contains("RGB 8-bit")
    );
}
