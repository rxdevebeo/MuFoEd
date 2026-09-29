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
//! - `render <file> [--out <dir|file.svg>] [--pages <range>] [--scale <n>]
//!   [--no-floating]` — renders Stage-4/5B SVG pages (floating heavy objects can
//!   be disabled with `--no-floating`).
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
//!
//! `render` returns `0` on success, `1` when the document was rendered but the
//! Feature Report has `unsupported`/`error` blockers, and `2` on failure.

use std::path::Path;
use std::process::ExitCode;

use strict_ooxml::{
    ConformancePolicy, Feature, FeatureStatus, Location, PageSelection, RenderOptions,
    StrictDocument,
};
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
        Some("render") => run_render(&args.collect::<Vec<_>>()),
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
        "usage: strict-ooxml <inspect|check|report|render> <file.docx> [--json|--text] [--out <path>] [--pages 1-3] [--scale 96] [--no-floating]"
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

/// Parsed `render` command arguments.
struct RenderArgs {
    file: String,
    out: Option<String>,
    pages: Option<PageSelection>,
    scale: Option<f64>,
    floating: bool,
}

impl RenderArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut file: Option<String> = None;
        let mut out: Option<String> = None;
        let mut pages: Option<PageSelection> = None;
        let mut scale: Option<f64> = None;
        let mut floating = true;
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--no-floating" => floating = false,
                "--out" => {
                    index += 1;
                    out = Some(args.get(index).ok_or("'--out' requires a path")?.clone());
                }
                "--pages" => {
                    index += 1;
                    pages = Some(
                        args.get(index)
                            .and_then(|value| parse_pages(value))
                            .ok_or("'--pages' expects a page or range, e.g. 1-3")?,
                    );
                }
                "--scale" => {
                    index += 1;
                    scale = Some(
                        args.get(index)
                            .and_then(|value| value.parse::<f64>().ok())
                            .filter(|value| value.is_finite() && *value > 0.0)
                            .ok_or("'--scale' expects a positive number")?,
                    );
                }
                flag if flag.starts_with("--") => {
                    return Err(format!("unknown option '{flag}'"));
                }
                positional => {
                    if file.is_some() {
                        return Err("'render' accepts a single file".to_owned());
                    }
                    file = Some(positional.to_owned());
                }
            }
            index += 1;
        }
        Ok(Self {
            file: file.ok_or("'render' requires a package path")?,
            out,
            pages,
            scale,
            floating,
        })
    }
}

fn run_render(args: &[String]) -> ExitCode {
    let parsed = match RenderArgs::parse(args) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(EXIT_ERROR);
        }
    };
    let options = OpenOptions::default();
    match StrictDocument::open_path(&parsed.file, &options) {
        Ok(document) => render_parsed(&document, &parsed),
        Err(error @ StrictError::TransitionalNotSupported { .. }) => {
            eprintln!(
                "error: rendering Transitional documents requires Stage-6 normalization: {error}"
            );
            ExitCode::from(EXIT_ERROR)
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

/// Renders an already-opened document and writes the result.
fn render_parsed(document: &StrictDocument, parsed: &RenderArgs) -> ExitCode {
    let mut render_options = RenderOptions::default();
    if let Some(scale) = parsed.scale {
        render_options = render_options.scale(scale);
    }
    if let Some(pages) = parsed.pages {
        render_options = render_options.pages(pages);
    }
    render_options = render_options.floating(parsed.floating);
    let rendered = match document.render_svg(&render_options) {
        Ok(rendered) => rendered,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(EXIT_ERROR);
        }
    };
    if rendered.is_empty() {
        eprintln!("error: no pages selected");
        return ExitCode::from(EXIT_ERROR);
    }
    if let Some(path) = &parsed.out {
        if let Err(error) = write_render_output(path, &rendered) {
            eprintln!("error: {error}");
            return ExitCode::from(EXIT_ERROR);
        }
    } else {
        for page in &rendered {
            print!("{}", page.svg);
        }
    }
    let report = document.support_report();
    if report.has_critical_problems() {
        eprintln!(
            "warning: rendered with {} unsupported/error mechanism(s)",
            report.summary.unsupported + report.summary.error
        );
        ExitCode::from(EXIT_PROBLEM)
    } else {
        ExitCode::from(EXIT_OK)
    }
}

/// Parses `1` or `1-3` into a [`PageSelection`].
fn parse_pages(value: &str) -> Option<PageSelection> {
    if let Some((start, end)) = value.split_once('-') {
        let start = start.parse::<usize>().ok()?;
        let end = end.parse::<usize>().ok()?;
        if start == 0 || end < start {
            return None;
        }
        Some(PageSelection::Range { start, end })
    } else {
        let page = value.parse::<usize>().ok()?;
        if page == 0 {
            return None;
        }
        Some(PageSelection::Range {
            start: page,
            end: page,
        })
    }
}

/// Writes the rendered pages to a single file or a directory.
fn write_render_output(path: &str, pages: &[strict_ooxml::Page]) -> Result<(), String> {
    let path = Path::new(path);
    if path.extension().is_some() {
        if pages.len() != 1 {
            return Err("'--out <file>' requires exactly one page; select with --pages".to_owned());
        }
        return std::fs::write(path, &pages[0].svg).map_err(|error| error.to_string());
    }
    std::fs::create_dir_all(path).map_err(|error| error.to_string())?;
    for page in pages {
        let name = format!("page-{}.svg", page.index + 1);
        std::fs::write(path.join(name), &page.svg).map_err(|error| error.to_string())?;
    }
    Ok(())
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
