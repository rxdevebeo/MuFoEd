//! `strict-ooxml` command-line interface.
//!
//! Commands:
//!
//! - `inspect <file>` — prints conformance, the part map and the relationship
//!   graph (opens with [`ConformancePolicy::Permissive`] so Transitional
//!   packages can be inspected).
//! - `check <file>` — validates and parses a Strict package under
//!   [`ConformancePolicy::StrictOnly`] and prints a brief support summary.
//! - `report <file> [--json|--text] [--out <path>]` — emits the full Stage-3
//!   Feature Report.
//!
//! Exit codes for `check` (per `TZ-STRICT-OOXML-RUST.md` decision G.8 and
//! `STAGE-3-TASK.md` §7.2):
//!
//! - `0` — Strict, parsed, no `unsupported`/`error` blocker;
//! - `1` — at least one `unsupported`/`error` blocker, or Transitional under
//!   `StrictOnly`;
//! - `2` — damaged input, undetermined conformance or internal error.
//!
//! `report` returns `0` on success and `2` on any failure (including
//! Transitional input, which Stage 3 refuses to report).

use std::process::ExitCode;

use strict_ooxml::{ConformancePolicy, Feature, FeatureStatus, Location, StrictDocument};
use strict_ooxml_core::error::StrictError;
use strict_ooxml_core::ns::Conformance;
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_core::part::PartId;

/// Exit code: no critical problem.
const EXIT_OK: u8 = 0;
/// Exit code: critical problem or Transitional under `StrictOnly`.
const EXIT_PROBLEM: u8 = 1;
/// Exit code: damaged input or internal error.
const EXIT_ERROR: u8 = 2;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("inspect") => run_inspect(&args.collect::<Vec<_>>()),
        Some("check") => run_check(&args.collect::<Vec<_>>()),
        Some("report") => run_report(&args.collect::<Vec<_>>()),
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
    eprintln!(
        "usage: strict-ooxml <inspect|check|report> <file.docx> [--json|--text] [--out <path>]"
    );
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
    match StrictDocument::open_path(path, &options) {
        Ok(document) => match document.package().conformance() {
            Conformance::Strict => {
                let report = document.support_report();
                // A blocker is an `unsupported` or `error` feature (ADR-0005).
                // `partial` is a warning and does not change the exit code.
                let blockers: Vec<&Feature> = report
                    .features
                    .iter()
                    .filter(|feature| {
                        matches!(
                            feature.status,
                            FeatureStatus::Unsupported | FeatureStatus::Error
                        )
                    })
                    .collect();
                if blockers.is_empty() {
                    println!("ok: strict");
                } else {
                    println!("strict: {} blocker(s) require attention", blockers.len());
                }
                println!("overall: {}", report.overall_status);
                let summary = report.summary;
                println!(
                    "summary: supported={} partial={} unsupported={} ignored={} error={}",
                    summary.supported,
                    summary.partial,
                    summary.unsupported,
                    summary.ignored,
                    summary.error,
                );
                for feature in &blockers {
                    let location = feature.locations.first().map_or("", Location::as_str);
                    println!(
                        "blocker: {} [{}] @ {location}",
                        feature.feature_id, feature.status
                    );
                }
                if blockers.is_empty() {
                    ExitCode::from(EXIT_OK)
                } else {
                    ExitCode::from(EXIT_PROBLEM)
                }
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
            ExitCode::from(EXIT_PROBLEM)
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

fn run_report(args: &[String]) -> ExitCode {
    let mut file: Option<&str> = None;
    let mut text = false;
    let mut out: Option<&str> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" => text = false,
            "--text" => text = true,
            "--out" => {
                index += 1;
                let Some(path) = args.get(index) else {
                    eprintln!("error: '--out' requires a path");
                    return ExitCode::from(EXIT_ERROR);
                };
                out = Some(path.as_str());
            }
            flag if flag.starts_with("--") => {
                eprintln!("error: unknown option '{flag}'");
                return ExitCode::from(EXIT_ERROR);
            }
            positional => {
                if file.is_some() {
                    eprintln!("error: 'report' accepts a single file");
                    return ExitCode::from(EXIT_ERROR);
                }
                file = Some(positional);
            }
        }
        index += 1;
    }
    let Some(file) = file else {
        eprintln!("error: 'report' requires a package path");
        return ExitCode::from(EXIT_ERROR);
    };

    let options = OpenOptions::default();
    match StrictDocument::open_path(file, &options) {
        Ok(document) => {
            let rendered = if text {
                document.report_text()
            } else {
                document.report_json()
            };
            if let Some(path) = out {
                if let Err(error) = std::fs::write(path, rendered) {
                    eprintln!("error: cannot write '{path}': {error}");
                    return ExitCode::from(EXIT_ERROR);
                }
                ExitCode::from(EXIT_OK)
            } else {
                print!("{rendered}");
                ExitCode::from(EXIT_OK)
            }
        }
        Err(error @ StrictError::TransitionalNotSupported { .. }) => {
            eprintln!(
                "error: report for Transitional documents requires Stage-6 normalization: {error}"
            );
            ExitCode::from(EXIT_ERROR)
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
