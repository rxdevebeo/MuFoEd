//! R09 browser acceptance for F07 (loaded face) and F20 (loss UI after JS).
//!
//! The TCP/API witness [`f20_successful_view_exposes_normalization_loss`] stays
//! in `f20_view.rs`. This file drives a pinned Chromium-family browser through
//! `xtool/browser-gate`. Missing runtime is FAIL with a BLOCKED message, never
//! a silent pass on static HTML.

#![allow(
    clippy::expect_used,
    clippy::print_stdout,
    clippy::use_debug,
    clippy::doc_markdown
)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use sha2::{Digest, Sha256};
use strict_ooxml_testkit::DocxBuilder;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn prepare_fixtures(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).expect("fixtures dir");

    let vml = strict_ooxml_testkit::audit::vml_loss_docx();
    std::fs::write(dir.join("vml-loss.docx"), &vml).expect("vml");
    std::fs::write(
        dir.join("vml-loss.docx.report.json"),
        r#"{"version":1,"output_sha256":"0000000000000000000000000000000000000000000000000000000000000000","outcome":"clean","issues":[]}"#,
    )
    .expect("stale sidecar");

    let plain = DocxBuilder::strict()
        .body("<w:p><w:r><w:t>Plain clean document</w:t></w:r></w:p>")
        .build();
    std::fs::write(dir.join("plain.docx"), plain).expect("plain");

    let many = DocxBuilder::strict()
        .body("<w:p><w:r><w:t>Sidecar host</w:t></w:r></w:p>")
        .build();
    let many_hash = sha256_hex(&many);
    std::fs::write(dir.join("sidecar-many.docx"), &many).expect("many");
    let issues = r#"[
      {"stage":"write","id":"many-1","severity":"lossy","count":1,"detail":"<script>alert(1)</script> & \"quotes\" #","part":"word/document.xml"},
      {"stage":"write","id":"many-2","severity":"lossy","count":1,"detail":"synthetic loss detail 2","part":"word/document.xml"},
      {"stage":"write","id":"many-3","severity":"lossy","count":1,"detail":"synthetic loss detail 3","part":"word/document.xml"},
      {"stage":"write","id":"many-4","severity":"lossy","count":1,"detail":"synthetic loss detail 4","part":"word/document.xml"},
      {"stage":"write","id":"many-5","severity":"lossy","count":1,"detail":"synthetic loss detail 5","part":"word/document.xml"},
      {"stage":"write","id":"many-6","severity":"lossy","count":1,"detail":"synthetic loss detail 6","part":"word/document.xml"}
    ]"#;
    let sidecar = format!(
        r#"{{"version":1,"output_sha256":"{many_hash}","outcome":"degraded","issues":{issues}}}"#
    );
    std::fs::write(dir.join("sidecar-many.docx.report.json"), sidecar).expect("matched sidecar");

    std::fs::write(dir.join("broken.docx"), b"not-a-zip").expect("broken");

    let face = DocxBuilder::strict()
        .body(
            "<w:p><w:r><w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\"/></w:rPr>\
<w:t>Face Title Selectable</w:t></w:r></w:p>",
        )
        .build();
    std::fs::write(dir.join("face.docx"), face).expect("face");
}

fn run_gate(report: &Path, fixtures: &Path, scenarios: &str) -> (i32, String) {
    prepare_fixtures(fixtures);
    let root = repo_root();
    let output = Command::new("python")
        .arg(root.join("xtool/browser-gate/run.py"))
        .arg("--viewer")
        .arg(env!("CARGO_BIN_EXE_strict-ooxml-view"))
        .arg("--report")
        .arg(report)
        .arg("--scenarios")
        .arg(scenarios)
        .arg("--fixtures-dir")
        .arg(fixtures)
        .current_dir(&root)
        .output()
        .expect("spawn browser-gate");
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if report.is_file() {
        text.push('\n');
        text.push_str(&std::fs::read_to_string(report).unwrap_or_default());
    }
    (output.status.code().unwrap_or(1), text)
}

#[test]
fn f20_browser_dom_after_js_exposes_loss_matrix() {
    let dir = repo_root().join("target/audit-rework-2026-10-05/R09/green");
    let _ = std::fs::create_dir_all(&dir);
    let report = dir.join("f20-browser-report.json");
    let fixtures = dir.join("f20-fixtures");
    let (code, text) = run_gate(&report, &fixtures, "f20-matrix");
    std::fs::write(dir.join("f20-browser.log"), &text).expect("log");
    assert_ne!(
        code, 3,
        "BLOCKED: browser runtime missing for F20 matrix\n{text}"
    );
    assert_eq!(code, 0, "F20 browser matrix failed\n{text}");
    let body = std::fs::read_to_string(&report).expect("report");
    assert!(body.contains("PASS"), "{body}");
    assert!(body.contains("f20-degraded-rejected-sidecar"), "{body}");
    assert!(body.contains("f20-clean"), "{body}");
    assert!(body.contains("f20-failed-open"), "{body}");
    assert!(body.contains("f20-matched-sidecar"), "{body}");
}

#[test]
fn f07_browser_loads_bundled_face_bytes_and_hash() {
    let dir = repo_root().join("target/audit-rework-2026-10-05/R09/green");
    let _ = std::fs::create_dir_all(&dir);
    let report = dir.join("f07-browser-report.json");
    let fixtures = dir.join("f07-fixtures");
    let (code, text) = run_gate(&report, &fixtures, "f07-face");
    std::fs::write(dir.join("f07-browser.log"), &text).expect("log");
    assert_ne!(
        code, 3,
        "BLOCKED: browser runtime missing for F07 face load\n{text}"
    );
    assert_eq!(code, 0, "F07 browser face load failed\n{text}");
    let body = std::fs::read_to_string(&report).expect("report");
    assert!(body.contains("PASS"), "{body}");
    assert!(body.contains("f07-browser-face-load"), "{body}");
    let carlito = std::fs::read(
        repo_root().join("strict-ooxml-render-svg/assets/fonts/carlito/Carlito-Regular.ttf"),
    )
    .expect("carlito");
    let expected = sha256_hex(&carlito);
    assert!(
        body.contains(&expected),
        "report must record R05 Carlito resource hash {expected}\n{body}"
    );
}

/// Inverse: TCP GET of `/` without JS must not already show live loss details.
#[test]
fn inverse_static_html_shell_has_empty_losses_until_js() {
    let dir = std::env::temp_dir().join(format!("strict-view-f20-inv-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(
        dir.join("vml-loss.docx"),
        strict_ooxml_testkit::audit::vml_loss_docx(),
    )
    .expect("docx");
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);
        port
    };
    let mut server = Command::new(env!("CARGO_BIN_EXE_strict-ooxml-view"))
        .arg(&dir)
        .args(["--port", &port.to_string(), "--transitional"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");
    for _ in 0..50 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    write!(
        stream,
        "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .expect("write");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read");
    let body = response.split_once("\r\n\r\n").map_or("", |(_, body)| body);
    assert!(body.contains("id=\"losses\""), "shell must include #losses");
    assert!(
        !body.contains("degraded ·")
            && !body.contains("sidecar rejected")
            && !body.contains("KEEP THIS TEXT"),
        "static HTML must not contain the live filled loss UI"
    );
    assert!(
        body.contains("showPipeline") && body.contains("pipeline.sidecar"),
        "JS must be what binds the live sidecar/outcome"
    );
    let _ = server.kill();
    let _ = server.wait();
    let _ = std::fs::remove_dir_all(&dir);

    let inv = repo_root().join("target/audit-rework-2026-10-05/R09/inverse");
    let _ = std::fs::create_dir_all(&inv);
    std::fs::write(
        inv.join("static-html-inverse.log"),
        "PASS: static shell has empty losses until JS\n",
    )
    .expect("inverse log");
}
