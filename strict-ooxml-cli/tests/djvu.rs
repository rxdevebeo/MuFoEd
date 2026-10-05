//! `DjVu` command argument, text import and output checks.
#[path = "../../strict-ooxml-djvu/tests/common/mod.rs"]
mod common;
use std::process::Command;
fn dir() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "strict-djvu-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&path).expect("temp");
    path
}
#[test]
fn cli_imports_djvu_and_dump_names_text_state() {
    let dir = dir();
    let input = dir.join("input.djvu");
    let output = dir.join("output.docx");
    std::fs::write(&input, common::text_file("Native text", true)).expect("fixture");
    let result = Command::new(env!("CARGO_BIN_EXE_strict-ooxml"))
        .arg("from-djvu")
        .arg(&input)
        .arg("--out")
        .arg(&output)
        .output()
        .expect("CLI");
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.is_file());
    let result = Command::new(env!("CARGO_BIN_EXE_strict-ooxml"))
        .arg("djvu-dump")
        .arg(&input)
        .output()
        .expect("dump");
    assert!(result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("text readable"));
    std::fs::remove_dir_all(dir).expect("cleanup test-owned directory");
}
#[test]
fn cli_rejects_invalid_options_without_writing_output() {
    let dir = dir();
    let input = dir.join("input.djvu");
    std::fs::write(&input, common::text_file("text", false)).expect("fixture");
    for args in [
        vec!["--mode", "invalid"],
        vec!["--pages", "0-2"],
        vec!["--ocr-model", "model"],
        vec!["--unexpected"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_strict-ooxml"))
            .arg("from-djvu")
            .arg(&input)
            .args(args)
            .arg("--out")
            .arg(dir.join("missing.docx"))
            .output()
            .expect("CLI");
        assert_eq!(result.status.code(), Some(2));
        assert!(!dir.join("missing.docx").exists());
    }
    std::fs::remove_dir_all(dir).expect("cleanup test-owned directory");
}
#[cfg(not(feature = "ocr-ollama"))]
#[test]
fn offline_build_reports_unsupported_ocr() {
    let result = Command::new(env!("CARGO_BIN_EXE_strict-ooxml"))
        .args([
            "from-djvu",
            "missing.djvu",
            "--out",
            "missing.docx",
            "--ocr",
        ])
        .output()
        .expect("CLI");
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("build with --features ocr-ollama"));
}
