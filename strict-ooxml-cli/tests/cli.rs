//! CLI integration tests: exit codes for `check` and output for `inspect`
//! (rework R8/R11).

#![allow(clippy::cast_possible_truncation)]

use std::path::PathBuf;
use std::process::Command;

const STRICT_W_NS: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
const TRANSITIONAL_W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const STRICT_DOC_REL: &str =
    "http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument";
const TRANSITIONAL_DOC_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";

const CONTENT_TYPES: &str = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;

fn root_rels(doc_rel: &str) -> String {
    format!(
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{doc_rel}" Target="word/document.xml"/></Relationships>"#
    )
}

fn document(ns: &str) -> String {
    format!(r#"<?xml version="1.0"?><w:document xmlns:w="{ns}"><w:body/></w:document>"#)
}

fn document_with_body(ns: &str, body: &str) -> String {
    format!(
        r#"<?xml version="1.0"?><w:document xmlns:w="{ns}"><w:body>{body}</w:body></w:document>"#
    )
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Builds a ZIP archive with stored (uncompressed) entries.
fn build_stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut local = Vec::new();
    let mut central = Vec::new();
    let mut offsets = Vec::new();
    for (name, content) in entries {
        offsets.push(local.len() as u32);
        let crc = crc32(content);
        local.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
        local.extend_from_slice(&20u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&crc.to_le_bytes());
        local.extend_from_slice(&(content.len() as u32).to_le_bytes());
        local.extend_from_slice(&(content.len() as u32).to_le_bytes());
        local.extend_from_slice(&(name.len() as u16).to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(name.as_bytes());
        local.extend_from_slice(content);
    }
    let cd_offset = local.len() as u32;
    for ((name, content), offset) in entries.iter().zip(offsets) {
        let crc = crc32(content);
        central.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(content.len() as u32).to_le_bytes());
        central.extend_from_slice(&(content.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let cd_size = central.len() as u32;
    let mut out = local;
    out.extend_from_slice(&central);
    out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("strict-ooxml-cli-{}-{name}", std::process::id()));
    std::fs::write(&path, bytes).expect("write temp file");
    path
}

fn run(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_strict-ooxml"))
        .args(args)
        .output()
        .expect("run binary");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn strict_docx() -> Vec<u8> {
    strict_docx_with_body("")
}

fn strict_docx_with_body(body: &str) -> Vec<u8> {
    let doc = document_with_body(STRICT_W_NS, body);
    build_stored_zip(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ("_rels/.rels", root_rels(STRICT_DOC_REL).as_bytes()),
        ("word/document.xml", doc.as_bytes()),
    ])
}

fn transitional_docx() -> Vec<u8> {
    let doc = document(TRANSITIONAL_W_NS);
    build_stored_zip(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ("_rels/.rels", root_rels(TRANSITIONAL_DOC_REL).as_bytes()),
        ("word/document.xml", doc.as_bytes()),
    ])
}

#[test]
fn check_strict_returns_zero() {
    let path = write_temp("strict.docx", &strict_docx());
    let (code, stdout, _) = run(&["check", path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(code, 0, "stdout: {stdout}");
    assert!(stdout.contains("ok: strict"), "stdout: {stdout}");
}

#[test]
fn check_transitional_returns_one() {
    let path = write_temp("transitional.docx", &transitional_docx());
    let (code, stdout, _) = run(&["check", path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(code, 1, "stdout: {stdout}");
    assert!(stdout.contains("transitional"), "stdout: {stdout}");
}

#[test]
fn check_transitional_with_flag_normalizes_and_returns_zero() {
    // AUD-23 / ADR-0016: the success line now names the T0-detected
    // conformance rather than always saying "strict", so a normalized
    // package is distinguishable from one that was already Strict.
    let path = write_temp("transitional-flag.docx", &transitional_docx());
    let (code, stdout, _) = run(&["check", path.to_str().unwrap(), "--transitional"]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(code, 0, "stdout: {stdout}");
    assert!(
        stdout.contains("ok: normalized from transitional"),
        "stdout: {stdout}"
    );
}

#[test]
fn check_damaged_returns_two() {
    let path = write_temp("damaged.docx", b"not a zip at all");
    let (code, _, stderr) = run(&["check", path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(code, 2, "stderr: {stderr}");
}

#[test]
fn inspect_prints_conformance_and_parts() {
    let path = write_temp("inspect.docx", &strict_docx());
    let (code, stdout, _) = run(&["inspect", path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(code, 0, "stdout: {stdout}");
    assert!(stdout.contains("conformance: Strict"), "stdout: {stdout}");
    assert!(stdout.contains("/word/document.xml"), "stdout: {stdout}");
    assert!(stdout.contains("relationships:"), "stdout: {stdout}");
}

#[test]
fn check_rejects_an_unrecognized_office_document_relationship() {
    // Before AUD-22, `RelType::from_uri` matched a URI's trailing path
    // segment, so this bogus `officeDocument` relationship still resolved as
    // the main document and left the package with no conformance signal at
    // all - `check` reported `Conformance::Unknown`. `from_uri` now requires
    // an exact match against the real Transitional or Strict URI, so this
    // relationship no longer resolves to anything, and the package has no
    // main document to open at all. This is also why `Unknown` can no longer
    // happen for a package that *does* open: the only two URIs that resolve
    // as the main document both carry a conformance signal of their own, so
    // an openable package is always at least Strict, Transitional or Mixed.
    let doc = document("urn:custom");
    let bytes = build_stored_zip(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        (
            "_rels/.rels",
            root_rels("https://example.invalid/officeDocument").as_bytes(),
        ),
        ("word/document.xml", doc.as_bytes()),
    ]);
    let path = write_temp("unknown.docx", &bytes);
    let (code, stdout, stderr) = run(&["check", path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(code, 2, "stdout: {stdout} stderr: {stderr}");
    assert!(!stdout.contains("ok: strict"), "stdout: {stdout}");
    assert!(stderr.contains("officeDocument"), "stderr: {stderr}");
}

#[test]
fn check_unsupported_mechanism_returns_one() {
    let path = write_temp("unsupported.docx", &strict_docx_with_body("<w:altChunk/>"));
    let (code, stdout, _) = run(&["check", path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(code, 1, "stdout: {stdout}");
    assert!(!stdout.contains("ok: strict"), "stdout: {stdout}");
    assert!(
        stdout.contains("blocker(s) require attention"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("unsupported=1"), "stdout: {stdout}");
    assert!(
        stdout.contains("blocker: w:altChunk [unsupported]"),
        "stdout: {stdout}"
    );
}

#[test]
fn report_json_by_default() {
    let path = write_temp("report-json.docx", &strict_docx_with_body("<w:altChunk/>"));
    let (code, stdout, _) = run(&["report", path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(code, 0, "stdout: {stdout}");
    assert!(stdout.contains("\"schema_version\": \"2.0\""), "{stdout}");
    assert!(
        stdout.contains("\"feature_id\": \"w:altChunk\""),
        "{stdout}"
    );
    assert!(stdout.ends_with('\n'), "{stdout}");
}

#[test]
fn report_text_flag() {
    let path = write_temp("report-text.docx", &strict_docx());
    let (code, stdout, _) = run(&["report", path.to_str().unwrap(), "--text"]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(code, 0, "stdout: {stdout}");
    assert!(stdout.contains("Feature report"), "{stdout}");
    assert!(stdout.contains("overall:"), "{stdout}");
}

#[test]
fn report_out_writes_file() {
    let docx = write_temp("report-out.docx", &strict_docx());
    let out = std::env::temp_dir().join(format!("strict-ooxml-report-{}.json", std::process::id()));
    let (code, stdout, _) = run(&[
        "report",
        docx.to_str().unwrap(),
        "--json",
        "--out",
        out.to_str().unwrap(),
    ]);
    let _ = std::fs::remove_file(&docx);
    assert_eq!(code, 0, "stdout: {stdout}");
    let written = std::fs::read_to_string(&out).expect("written report");
    let _ = std::fs::remove_file(&out);
    assert!(written.contains("\"schema_version\": \"2.0\""), "{written}");
}

#[test]
fn report_transitional_returns_two() {
    let path = write_temp("report-transitional.docx", &transitional_docx());
    let (code, _stdout, stderr) = run(&["report", path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(code, 2, "stderr: {stderr}");
    assert!(stderr.contains("Transitional"), "stderr: {stderr}");
}

#[test]
fn report_damaged_returns_two() {
    let path = write_temp("report-damaged.docx", b"not a zip at all");
    let (code, _stdout, stderr) = run(&["report", path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(code, 2, "stderr: {stderr}");
}

#[test]
fn report_missing_file_argument_returns_two() {
    let (code, _stdout, stderr) = run(&["report", "--json"]);
    assert_eq!(code, 2, "stderr: {stderr}");
}

#[test]
fn render_prints_svg() {
    let path = write_temp(
        "render.docx",
        &strict_docx_with_body("<w:p><w:r><w:t>Hello</w:t></w:r></w:p>"),
    );
    let (code, stdout, _) = run(&["render", path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(code, 0, "stdout: {stdout}");
    assert!(stdout.contains("<svg "), "{stdout}");
    assert!(stdout.contains("Hello"), "{stdout}");
}

#[test]
fn render_unsupported_mechanism_returns_one() {
    let path = write_temp(
        "render-unsupported.docx",
        &strict_docx_with_body("<w:p><w:r><w:t>x</w:t></w:r></w:p><w:altChunk/>"),
    );
    let (code, stdout, stderr) = run(&["render", path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(code, 1, "stdout: {stdout} stderr: {stderr}");
    assert!(stdout.contains("<svg "), "{stdout}");
}

#[test]
fn render_out_directory_writes_pages() {
    let docx = write_temp("render-dir.docx", &strict_docx_with_body("<w:p/>"));
    let dir = std::env::temp_dir().join(format!("strict-ooxml-render-{}", std::process::id()));
    let (code, _stdout, stderr) = run(&[
        "render",
        docx.to_str().unwrap(),
        "--out",
        dir.to_str().unwrap(),
    ]);
    let _ = std::fs::remove_file(&docx);
    assert_eq!(code, 0, "stderr: {stderr}");
    let page = dir.join("page-1.svg");
    let svg = std::fs::read_to_string(&page).expect("page file");
    assert!(svg.contains("<svg "), "{svg}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn render_out_file_requires_single_page() {
    let docx = write_temp("render-file.docx", &strict_docx_with_body("<w:p/>"));
    let out = std::env::temp_dir().join(format!("strict-ooxml-page-{}.svg", std::process::id()));
    let (code, _stdout, stderr) = run(&[
        "render",
        docx.to_str().unwrap(),
        "--pages",
        "1",
        "--out",
        out.to_str().unwrap(),
    ]);
    let _ = std::fs::remove_file(&docx);
    assert_eq!(code, 0, "stderr: {stderr}");
    let svg = std::fs::read_to_string(&out).expect("svg file");
    let _ = std::fs::remove_file(&out);
    assert!(svg.contains("<svg "), "{svg}");
}

#[test]
fn render_transitional_returns_two() {
    let path = write_temp("render-transitional.docx", &transitional_docx());
    let (code, _stdout, stderr) = run(&["render", path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(code, 2, "stderr: {stderr}");
}

#[test]
fn render_damaged_returns_two() {
    let path = write_temp("render-damaged.docx", b"not a zip");
    let (code, _stdout, stderr) = run(&["render", path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(code, 2, "stderr: {stderr}");
}
