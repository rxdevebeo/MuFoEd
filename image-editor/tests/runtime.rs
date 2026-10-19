//! Cross-backend lifecycle checks without installed inference runtimes or models.
use image_editor::{
    generation::{Job, Mode, Request},
    model_tools::ModelJob,
    ui::ImageEditor,
    Document,
};
use mufoed_model_runtime::{Policy, Runtime, Task};

fn request() -> Request {
    serde_json::from_value(serde_json::json!({
        "mode": "TextToImage", "model": "sd15", "prompt": "test", "negative": "",
        "seed": 1, "steps": 1, "guidance": 1.0, "strength": 0.5
    }))
    .unwrap()
}

#[test]
fn text_lease_blocks_cli_and_python_until_their_queue_entries_are_cancelled() {
    let runtime = Runtime::new(Policy::default());
    let mut text = runtime
        .enqueue(Task::new("text", "ollama-adapter-test"))
        .unwrap();
    let text_lease = text.try_start().unwrap().unwrap();
    let doc = Document::new(64, 64).unwrap();
    let mut cli = Job::start_with_runtime(
        "nonexistent-plakat-test-executable",
        request(),
        &doc,
        &runtime,
    )
    .unwrap();
    let mut python = ModelJob::start_with_runtime(
        "nonexistent-runtime-test.py",
        &[],
        Some(&doc.flatten()),
        None,
        &runtime,
    )
    .unwrap();
    // Neither missing executable/script is started while text holds admission.
    assert!(cli.poll().unwrap().is_none());
    assert!(python.poll().unwrap().is_none());
    assert_eq!(runtime.snapshot().queued.len(), 2);
    cli.cancel();
    drop(python);
    assert!(runtime.snapshot().queued.is_empty());
    assert_eq!(runtime.snapshot().active.unwrap().owner, "text");
    drop(text_lease);
    assert!(runtime.snapshot().active.is_none());
}

#[test]
fn cli_spawn_error_releases_admission_for_the_next_backend() {
    let runtime = Runtime::new(Policy::default());
    let doc = Document::new(64, 64).unwrap();
    let mut job = Job::start_with_runtime(
        "nonexistent-plakat-test-executable",
        request(),
        &doc,
        &runtime,
    )
    .unwrap();
    assert!(job.poll().is_err());
    assert!(runtime.snapshot().active.is_none());
    let mut next = runtime.enqueue(Task::new("text", "next")).unwrap();
    assert!(next.try_start().unwrap().is_some());
}

#[test]
fn editor_replacement_preserves_host_runtime() {
    let runtime = Runtime::new(Policy::default());
    let mut text = runtime.enqueue(Task::new("text", "host-model")).unwrap();
    let lease = text.try_start().unwrap().unwrap();
    let mut editor = ImageEditor::with_runtime(Document::new(64, 64).unwrap(), runtime.clone());
    editor.set_generation_mode(Mode::ImageToImage);
    editor.replace(Document::new(128, 64).unwrap());
    assert_eq!(editor.document().width(), 128);
    let mut image = editor
        .runtime()
        .enqueue(Task::new("image", "next"))
        .unwrap();
    assert!(image.try_start().unwrap().is_none());
    assert_eq!(runtime.snapshot().queued.len(), 1);
    drop(lease);
    assert!(image.try_start().unwrap().is_some());
}
