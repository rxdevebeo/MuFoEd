//! `strict-ooxml` command-line interface.
//!
//! Stage 1 provides two commands:
//!
//! - `inspect <file>` — prints conformance, the part map and the relationship
//!   graph (opens with [`ConformancePolicy::Permissive`] so Transitional
//!   packages can be inspected).
//! - `check <file>` — validates a Strict package under
//!   [`ConformancePolicy::StrictOnly`].
//!
//! Exit codes for `check` (per `TZ-STRICT-OOXML-RUST.md` decision G.8):
//!
//! - `0` — Strict OK;
//! - `1` — Transitional detected under `StrictOnly`;
//! - `2` — damaged input or internal error.

use std::process::ExitCode;

use strict_ooxml_core::error::StrictError;
use strict_ooxml_core::ns::Conformance;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_core::part::PartId;

/// `check` exit code: the package is valid Strict.
const EXIT_OK: u8 = 0;
/// `check` exit code: the package is Transitional and `StrictOnly` applies.
const EXIT_TRANSITIONAL: u8 = 1;
/// `check` exit code: the package is damaged or an internal error occurred.
const EXIT_ERROR: u8 = 2;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("inspect") => run_inspect(&args.collect::<Vec<_>>()),
        Some("check") => run_check(&args.collect::<Vec<_>>()),
        Some("--help" | "-h") | None => {
            print_usage();
            ExitCode::from(EXIT_OK)
        }
        Some(other) => {
            eprintln!("error: unknown command '{other}'");
            print_usage();
            ExitCode::from(EXIT_ERROR)
        }
    }
}

fn print_usage() {
    eprintln!("usage: strict-ooxml <inspect|check> <file.docx>");
}

fn run_inspect(args: &[String]) -> ExitCode {
    let Some(path) = args.first() else {
        eprintln!("error: 'inspect' requires a package path");
        return ExitCode::from(EXIT_ERROR);
    };
    let options = OpenOptions::default().conformance(ConformancePolicy::Permissive);
    match Package::open_path(path, &options) {
        Ok(package) => {
            println!("conformance: {:?}", package.conformance());
            match package.main_document_part() {
                Ok(main) => println!("main document: {main}"),
                Err(error) => eprintln!("warning: {error}"),
            }
            println!("parts ({}):", package.parts().count());
            for part in package.parts() {
                let content_type = part.content_type.as_deref().unwrap_or("<unknown>");
                println!("  {} [{}] {content_type}", part.id, compression_name(part));
            }
            println!("relationships:");
            let root = PartId::new("/");
            print_relationships(&package, &root);
            for part in package.parts() {
                print_relationships(&package, &part.id);
            }
            ExitCode::from(EXIT_OK)
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

fn run_check(args: &[String]) -> ExitCode {
    let Some(path) = args.first() else {
        eprintln!("error: 'check' requires a package path");
        return ExitCode::from(EXIT_ERROR);
    };
    let options = OpenOptions::default();
    match Package::open_path(path, &options) {
        Ok(package) => match package.conformance() {
            Conformance::Strict => {
                println!("ok: strict");
                ExitCode::from(EXIT_OK)
            }
            Conformance::Unknown => {
                println!("unknown: conformance could not be determined");
                ExitCode::from(EXIT_ERROR)
            }
            other => {
                println!("ok: {other:?}");
                ExitCode::from(EXIT_OK)
            }
        },
        Err(error @ StrictError::TransitionalNotSupported { .. }) => {
            println!("transitional: {error}");
            ExitCode::from(EXIT_TRANSITIONAL)
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

fn compression_name(part: &strict_ooxml_core::part::Part) -> &'static str {
    match part.compression {
        strict_ooxml_core::part::Compression::Stored => "stored",
        strict_ooxml_core::part::Compression::Deflate => "deflate",
    }
}

fn print_relationships(package: &Package, source: &PartId) {
    let relationships = package.relationships(source);
    if relationships.is_empty() {
        return;
    }
    println!("  {source}:");
    for rel in relationships {
        let target = match &rel.resolved {
            Some(resolved) => resolved.as_str().to_owned(),
            None => format!("{} (external)", rel.target),
        };
        println!("    {} -> {:?} [{target}]", rel.id, rel.rel_type);
    }
}
