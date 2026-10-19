//! Behavioral checks for editing, persistence, and inference request isolation.
use image_editor::{
    generation::{Mode, Request},
    Adjustment, Document, History, Raster, Tool,
};

#[test]
fn layered_composite_preserves_straight_alpha() {
    let mut doc = Document::from_raster(Raster::solid(8, 8, [255, 0, 0, 255]).unwrap()).unwrap();
    doc.add_layer(
        "Blue".into(),
        Raster::solid(8, 8, [0, 0, 255, 128]).unwrap(),
    )
    .unwrap();
    assert_eq!(&doc.flatten().pixels()[..4], &[127, 0, 128, 255]);
    doc.set_layer("Blue".into(), false, 255);
    assert_eq!(&doc.flatten().pixels()[..4], &[255, 0, 0, 255]);
}

#[test]
fn painting_is_continuous_and_undo_restores_pixels_and_mask() {
    let mut doc = Document::new(64, 64).unwrap();
    let original = doc.clone();
    let mut history = History::default();
    history.checkpoint(&doc);
    doc.stroke(
        [2.0, 32.0],
        [60.0, 32.0],
        2.0,
        [40, 80, 120, 255],
        Tool::Brush,
    )
    .unwrap();
    for x in 2..60 {
        assert_eq!(doc.flatten().pixels()[(32 * 64 + x) * 4 + 3], 255);
    }
    doc.stroke([10.0, 10.0], [20.0, 20.0], 3.0, [0; 4], Tool::Mask)
        .unwrap();
    let painted = doc.clone();
    assert!(history.undo(&mut doc));
    assert_eq!(doc, original);
    assert!(history.redo(&mut doc));
    assert_eq!(doc, painted);
    history.undo(&mut doc);
    history.checkpoint(&doc);
    assert!(!history.redo(&mut doc));
}

#[test]
fn eraser_removes_alpha_without_painting_a_background() {
    let mut doc = Document::from_raster(Raster::solid(8, 8, [0, 255, 0, 255]).unwrap()).unwrap();
    doc.stroke([4.0, 4.0], [4.0, 4.0], 2.0, [0, 0, 0, 255], Tool::Eraser)
        .unwrap();
    assert_eq!(doc.flatten().pixels()[(4 * 8 + 4) * 4 + 3], 0);
    assert_eq!(doc.flatten().pixels()[3], 255);
}

#[test]
fn photo_operations_keep_alpha_and_median_removes_an_impulse() {
    let mut pixels = [20, 20, 20, 123].repeat(9);
    pixels[16..20].copy_from_slice(&[255, 255, 255, 123]);
    let mut doc = Document::from_raster(Raster::new(3, 3, pixels).unwrap()).unwrap();
    doc.adjust(Adjustment::Median).unwrap();
    assert_eq!(
        &doc.layers()[0].raster().pixels()[16..20],
        &[20, 20, 20, 123]
    );
    doc.adjust(Adjustment::Brightness(10)).unwrap();
    assert_eq!(&doc.layers()[0].raster().pixels()[..4], &[30, 30, 30, 123]);
    doc.adjust(Adjustment::Blur(1.0)).unwrap();
    assert!(doc.layers()[0]
        .raster()
        .pixels()
        .chunks_exact(4)
        .all(|p| p[3] == 123));
}

#[test]
fn crop_rotate_and_resize_transform_layers_and_mask_together() {
    let mut doc = Document::new(8, 16).unwrap();
    doc.stroke([2.5, 4.5], [2.5, 4.5], 0.5, [10, 20, 30, 255], Tool::Brush)
        .unwrap();
    doc.stroke([2.5, 4.5], [2.5, 4.5], 0.5, [0; 4], Tool::Mask)
        .unwrap();
    doc.crop(2, 4, 4, 8).unwrap();
    assert_eq!(doc.mask()[0], 255);
    doc.rotate90();
    assert_eq!((doc.width(), doc.height()), (8, 4));
    assert_eq!(doc.mask()[7], 255);
    doc.resize(16, 8).unwrap();
    assert_eq!(doc.mask()[14], 255);
    assert_eq!(doc.mask().len(), 128);
    assert!(doc.crop(u32::MAX, 0, 1, 1).is_err());
}

#[test]
fn self_contained_project_roundtrip_and_png_roundtrip() {
    let temp = tempfile::tempdir().unwrap();
    let mut doc = Document::from_raster(Raster::solid(8, 16, [15, 70, 200, 100]).unwrap()).unwrap();
    doc.add_generated_layer(
        "Overlay".into(),
        Raster::solid(8, 16, [200, 70, 15, 80]).unwrap(),
        Request {
            prompt: "test recipe retained across project reload".into(),
            ..Request::default()
        },
    )
    .unwrap();
    doc.stroke([4.0, 4.0], [4.0, 4.0], 2.0, [0; 4], Tool::Mask)
        .unwrap();
    let project = temp.path().join("project.json");
    doc.save_project(&project).unwrap();
    assert_eq!(Document::open_project(&project).unwrap(), doc);
    doc.save_project(&project).unwrap(); // replacing an existing project must work on Windows
    let png = temp.path().join("export.png");
    doc.flatten().save_png(&png).unwrap();
    assert_eq!(Raster::open(png).unwrap(), doc.flatten());
}

#[test]
fn invalid_projects_and_public_deserialization_cannot_break_invariants() {
    let mut data: serde_json::Value =
        serde_json::from_slice(&Document::new(8, 8).unwrap().to_json().unwrap()).unwrap();
    data["layers"][0]["raster"]["pixels"] = serde_json::json!([0, 0, 0, 0]);
    assert!(Document::from_json(&serde_json::to_vec(&data).unwrap()).is_err());
    assert!(serde_json::from_value::<Document>(data).is_err());
    assert!(serde_json::from_str::<Raster>(r#"{"width":100,"height":100,"pixels":[]}"#).is_err());
    assert!(Document::new(0, 8).is_err());
    assert!(Document::new(u32::MAX, u32::MAX).is_err());
}

#[test]
fn invalid_operations_leave_document_unchanged() {
    let mut doc = Document::new(8, 8).unwrap();
    let original = doc.clone();
    assert!(doc
        .add_layer("bad".into(), Raster::solid(16, 16, [0; 4]).unwrap())
        .is_err());
    assert!(doc.adjust(Adjustment::Contrast(f32::NAN)).is_err());
    assert!(doc
        .stroke([f32::NAN, 0.0], [0.0, 0.0], 2.0, [0; 4], Tool::Brush)
        .is_err());
    assert!(doc.crop(0, 0, 9, 8).is_err());
    assert_eq!(doc, original);
}

#[test]
fn cli_keeps_prompt_and_paths_in_separate_arguments() {
    let doc = Document::new(64, 64).unwrap();
    let request = Request {
        prompt: "a vase; $(this is literal) `test`".into(),
        ..Request::default()
    };
    let args = request
        .arguments(&doc, std::path::Path::new("C:/path with spaces"))
        .unwrap();
    assert_eq!(args[0], "generate");
    assert_eq!(args[1], request.prompt.as_str());
    let expected = std::path::Path::new("C:/path with spaces").join("out");
    assert!(args.iter().any(|s| s == expected.as_os_str()));
}

#[test]
fn inpaint_requires_a_mask_and_exports_opaque_white_regeneration_region() {
    let mut doc = Document::new(64, 64).unwrap();
    let request = Request {
        mode: Mode::Inpaint,
        prompt: "a ceramic vase".into(),
        ..Request::default()
    };
    assert!(request.validate(&doc).is_err());
    doc.stroke([20.0, 20.0], [24.0, 24.0], 3.0, [0; 4], Tool::Mask)
        .unwrap();
    request.validate(&doc).unwrap();
    for (pixel, mask) in doc.mask_raster().pixels().chunks_exact(4).zip(doc.mask()) {
        assert_eq!(pixel, &[*mask, *mask, *mask, 255]);
    }
    let args = request
        .arguments(&doc, std::path::Path::new("work"))
        .unwrap();
    assert_eq!(args[0], "img2img");
    assert!(args.iter().any(|s| s == "--mask"));
}

#[test]
fn missing_backend_returns_an_actionable_error() {
    let doc = Document::new(64, 64).unwrap();
    let request = Request {
        prompt: "a ceramic vase".into(),
        ..Request::default()
    };
    let runtime = mufoed_model_runtime::Runtime::new(mufoed_model_runtime::Policy::default());
    let mut job = image_editor::generation::Job::start_with_runtime(
        "Z:/definitely-absent/plakat.exe",
        request,
        &doc,
        &runtime,
    )
    .unwrap();
    let err = job.poll().err().unwrap();
    assert!(err.to_string().contains("Cannot start Plakat"));
}
