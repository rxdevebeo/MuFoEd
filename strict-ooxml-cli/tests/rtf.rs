//! Process-level RTF outcome, report and source-preservation contracts.
use std::{
    path::{Path, PathBuf},
    process::Command,
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "strictlib-rtf-cli-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn run(input: &Path, output: &Path, extra: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_strict-ooxml"))
        .arg("import-rtf")
        .arg(input)
        .arg("--out")
        .arg(output)
        .args(extra)
        .output()
        .unwrap()
}
#[test]
fn process_exit_codes_and_persisted_report_match_fidelity() {
    let temp = Temp::new();
    let input = temp.0.join("source.rtf");
    let output = temp.0.join("output.docx");
    for (source, code, loss) in [
        (br"{\rtf1 Hello}".as_slice(), 0, 0),
        (br"{\rtf1 A\unsupported B}".as_slice(), 1, 1),
    ] {
        std::fs::write(&input, source).unwrap();
        let result = run(&input, &output, &[]);
        assert_eq!(
            result.status.code(),
            Some(code),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(std::fs::read(&input).unwrap(), source);
        let report: serde_json::Value = serde_json::from_slice(
            &std::fs::read(temp.0.join("output.docx.rtf-report.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(report["import"]["loss_count"], loss);
        assert_eq!(report["writer"]["loss_count"], 0);
        let p = strict_ooxml_core::opc::Package::open_path(
            &output,
            &strict_ooxml_core::opc::OpenOptions::default(),
        )
        .unwrap();
        assert_eq!(p.conformance(), strict_ooxml_core::ns::Conformance::Strict);
    }
    std::fs::remove_file(&output).unwrap();
    std::fs::write(&input, br"{\rtf1 truncated").unwrap();
    assert_eq!(run(&input, &output, &[]).status.code(), Some(2));
    assert!(!output.exists());
    std::fs::write(&input, br"{\rtf1\unsupported x}").unwrap();
    assert_eq!(
        run(&input, &output, &["--reject-unsupported"])
            .status
            .code(),
        Some(2)
    );
    assert!(!output.exists());
    let source = std::fs::read(&input).unwrap();
    assert_eq!(run(&input, &input, &[]).status.code(), Some(2));
    assert_eq!(std::fs::read(input).unwrap(), source);
}

#[test]
fn import_and_output_failures_persist_failed_reports() {
    let temp = Temp::new();
    let input = temp.0.join("source.rtf");
    let output = temp.0.join("output.docx");
    let report = temp.0.join("report.json");
    std::fs::write(&input, br"{\rtf1 truncated").unwrap();
    assert_eq!(
        run(&input, &output, &["--report", report.to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
    let failed: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&report).expect("fatal imports need a durable report"),
    )
    .unwrap();
    assert_eq!(failed["status"], "Failed");
    assert_eq!(failed["error"]["stage"], "Convert");
    assert!(!output.exists());
    std::fs::write(&input, br"{\rtf1 Hello}").unwrap();
    std::fs::create_dir(&output).unwrap();
    assert_eq!(
        run(&input, &output, &["--report", report.to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
    let failed: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
    assert_eq!(
        failed["status"], "Failed",
        "output IO must not leave a Clean report"
    );
    assert_eq!(failed["error"]["stage"], "Write");
    assert_eq!(failed["import"]["loss_count"], 0);
    assert!(output.is_dir());
    assert_eq!(std::fs::read(&input).unwrap(), br"{\rtf1 Hello}");
}
