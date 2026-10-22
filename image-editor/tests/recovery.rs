//! Durable separate recovery: exact data, OS ownership, malformed files and failed writes.
use image_editor::{
    generation::Request,
    recovery::{self, Session},
    Document, LayerBlendMode, Raster,
};
fn document() -> Document {
    let mut d = Document::from_generated(
        Raster::solid(8, 6, [10, 20, 30, 128]).unwrap(),
        Request {
            seed: 9876,
            ..Default::default()
        },
    )
    .unwrap();
    d.add_layer(
        "Свет 背景".into(),
        Raster::solid(8, 6, [200, 80, 40, 129]).unwrap(),
    )
    .unwrap();
    d.set_blend_mode(LayerBlendMode::Overlay).unwrap();
    d.add_layer_mask(true).unwrap();
    d.set_layer_mask([0, 64, 128, 255].repeat(12)).unwrap();
    d.set_layer_mask_enabled(false).unwrap();
    d.set_layer("Свет 背景".into(), false, 173);
    d.set_layer_locked(true);
    d.set_mask(vec![77; 48]).unwrap();
    d.set_selection([0, 128, 255, 0].repeat(12)).unwrap();
    d.select(0).unwrap();
    d
}
#[test]
fn abandoned_snapshot_retains_exact_document_and_origin_without_touching_psd() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("original.psd");
    let d = document();
    d.save_project(&source).unwrap();
    let original = std::fs::read(&source).unwrap();
    let directory = root.path().join("recovery");
    let slot = Session::new(&directory).unwrap();
    slot.checkpoint(&d, Some(&source)).unwrap();
    let path = slot.path().to_path_buf();
    assert!(
        recovery::discover(&directory).unwrap().is_empty(),
        "live window must be skipped"
    );
    drop(slot);
    let candidates = recovery::discover(&directory).unwrap();
    assert_eq!(candidates.len(), 1);
    assert!(
        recovery::discover(&directory).unwrap().is_empty(),
        "candidate already claimed"
    );
    let draft = candidates[0].load().unwrap();
    assert_eq!(draft.document, d);
    assert_eq!(draft.source, Some(source.clone()));
    assert!(draft.saved_at > 0);
    assert_eq!(std::fs::read(&source).unwrap(), original);
    drop(candidates);
    assert!(path.exists(), "defer preserves draft");
    let candidates = recovery::discover(&directory).unwrap();
    candidates[0].discard().unwrap();
    drop(candidates);
    assert!(!path.exists());
    assert!(recovery::discover(&directory).unwrap().is_empty());
    assert_eq!(std::fs::read(source).unwrap(), original);
}
#[test]
fn two_sessions_are_isolated_and_temporary_files_are_never_offered() {
    let root = tempfile::tempdir().unwrap();
    let a = Session::new(root.path()).unwrap();
    let b = Session::new(root.path()).unwrap();
    a.checkpoint(&document(), None).unwrap();
    b.checkpoint(&Document::new(2, 3).unwrap(), None).unwrap();
    assert_ne!(a.path(), b.path());
    assert!(recovery::discover(root.path()).unwrap().is_empty());
    let a_path = a.path().to_path_buf();
    a.clear().unwrap();
    drop(a);
    drop(b);
    std::fs::write(root.path().join("partial.tmp"), b"interrupted write").unwrap();
    std::fs::write(root.path().join("other.snapshot"), b"not ours").unwrap();
    let candidates = recovery::discover(root.path()).unwrap();
    assert_eq!(candidates.len(), 1);
    assert!(!a_path.exists());
    assert_eq!(
        candidates[0].load().unwrap().document,
        Document::new(2, 3).unwrap()
    );
}
#[test]
fn every_truncation_checksum_damage_and_extra_payload_are_errors() {
    let root = tempfile::tempdir().unwrap();
    let slot = Session::new(root.path()).unwrap();
    slot.checkpoint(&Document::new(2, 2).unwrap(), None)
        .unwrap();
    let path = slot.path().to_path_buf();
    drop(slot);
    let candidates = recovery::discover(root.path()).unwrap();
    let c = &candidates[0];
    let bytes = std::fs::read(&path).unwrap();
    for length in 0..bytes.len() {
        std::fs::write(&path, &bytes[..length]).unwrap();
        assert!(c.load().is_err(), "truncation {length}");
    }
    let mut damaged = bytes.clone();
    let i = damaged.len() - 8;
    damaged[i] ^= 1;
    std::fs::write(&path, &damaged).unwrap();
    assert!(c.load().is_err());
    let mut trailing = bytes.clone();
    trailing.extend_from_slice(b"garbage");
    std::fs::write(&path, trailing).unwrap();
    assert!(c.load().is_err());
    std::fs::write(&path, bytes).unwrap();
    assert_eq!(c.load().unwrap().document, Document::new(2, 2).unwrap());
}
#[test]
fn failed_metadata_serialization_keeps_previous_atomic_checkpoint_and_fingerprint_is_stable() {
    let root = tempfile::tempdir().unwrap();
    let slot = Session::new(root.path()).unwrap();
    let d = document();
    slot.checkpoint(&d, None).unwrap();
    let prior = std::fs::read(slot.path()).unwrap();
    let signature = recovery::fingerprint(&d, None).unwrap();
    assert_eq!(signature, recovery::fingerprint(&d, None).unwrap());
    assert_ne!(
        signature,
        recovery::fingerprint(&d, Some(std::path::Path::new("another.psd"))).unwrap()
    );
    let mut huge = d.clone();
    huge.set_layer("x".repeat(4 * 1024 * 1024), true, 255);
    assert!(slot.checkpoint(&huge, None).is_err());
    assert_eq!(std::fs::read(slot.path()).unwrap(), prior);
    let invalid = Document::from_generated(
        Raster::solid(2, 2, [0; 4]).unwrap(),
        Request {
            guidance: f32::NAN,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(slot.checkpoint(&invalid, None).is_err());
    assert_eq!(std::fs::read(slot.path()).unwrap(), prior);
}
