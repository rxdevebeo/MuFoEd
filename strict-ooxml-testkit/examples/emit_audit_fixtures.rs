//! Writes the audit scenarios into a directory the Python runner can hash.
//!
//! ```text
//! cargo +1.92.0 run -p strict-ooxml-testkit --example emit_audit_fixtures -- --out DIR
//! ```

#![allow(missing_docs)]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use strict_ooxml_testkit::audit::{
    damaged_crc_docx, damaged_truncated_docx, damaged_xml_docx, scenarios, ScenarioKind,
};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let mut out: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        if arg == "--out" {
            let Some(path) = args.next() else {
                eprintln!("error: --out requires a directory");
                return ExitCode::from(2);
            };
            out = Some(PathBuf::from(path));
        } else {
            eprintln!("error: unknown argument {arg}");
            return ExitCode::from(2);
        }
    }
    let Some(out) = out else {
        eprintln!("error: --out is required");
        return ExitCode::from(2);
    };
    if let Err(error) = fs::create_dir_all(&out) {
        eprintln!("error: {error}");
        return ExitCode::from(2);
    }
    for scenario in scenarios() {
        let ext = match scenario.kind {
            ScenarioKind::Docx => "docx",
            ScenarioKind::Pdf => "pdf",
        };
        let path = out.join(format!("{}-{}.{}", scenario.id, scenario.name, ext));
        if let Err(error) = fs::write(&path, (scenario.build)()) {
            eprintln!("error: {}: {error}", path.display());
            return ExitCode::from(2);
        }
    }
    for (name, bytes) in [
        ("damaged-xml.docx", damaged_xml_docx()),
        ("damaged-crc.docx", damaged_crc_docx()),
        ("damaged-truncated.docx", damaged_truncated_docx()),
    ] {
        if let Err(error) = fs::write(out.join(name), bytes) {
            eprintln!("error: {name}: {error}");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
