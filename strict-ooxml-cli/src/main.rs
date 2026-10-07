//! `strict-ooxml` command-line interface.
//!
//! Commands:
//!
//! - `inspect <file>` — prints conformance, the part map and the relationship
//!   graph (opens with [`ConformancePolicy::Permissive`], which accepts
//!   Strict and conformance-`Unknown` packages with no normalizer installed;
//!   a Transitional or Mixed package is refused, per the policy matrix in
//!   ADR-0016).
//! - `check <file>` — validates and parses a Strict package under
//!   [`ConformancePolicy::StrictOnly`] and prints a brief support summary.
//! - `report <file> [--json|--text] [--out <path>]` — emits the full Stage-3
//!   Feature Report.
//! - `render <file> [--out <dir|file.svg>] [--pages <range>] [--scale <n>]
//!   [--no-floating] [--no-math]` — renders Stage-4/5B/5C SVG pages (floating
//!   heavy objects and OMML formulas can be disabled with `--no-floating` and
//!   `--no-math`).
//! - `write <file.docx> --out <file.docx>` — serializes the parsed model back
//!   to a Strict package (Stage 8A) and prints what the writer could not
//!   express.
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

// Every line this tool prints goes through `terminal_safe`: part names,
// relationship ids and targets, content types, loss details and error messages
// all come from the document, and a raw ESC or BEL in them is a terminal
// escape sequence (title rewrite, colour, cursor movement), not text. These
// shadow the std macros for this file, so no call site can forget.
macro_rules! println {
    () => { std::println!() };
    ($($arg:tt)*) => { std::println!("{}", crate::terminal_safe(&format!($($arg)*))) };
}
macro_rules! eprintln {
    () => { std::eprintln!() };
    ($($arg:tt)*) => { std::eprintln!("{}", crate::terminal_safe(&format!($($arg)*))) };
}

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use strict_ooxml::{
    ConformancePolicy, Feature, FeatureStatus, Location, PageSelection, RenderOptions,
    StrictDocument, TransitionalNormalizer,
};
use strict_ooxml_core::error::StrictError;
use strict_ooxml_core::ns::Conformance;
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_core::pipeline::{PipelineIssue, PipelineOutcome, PipelineStage, PipelineSummary};

/// Exit code: no critical problem.
const EXIT_OK: u8 = 0;
/// Exit code: critical problem or Transitional under `StrictOnly`.
const EXIT_PROBLEM: u8 = 1;
/// Exit code: damaged input or internal error.
const EXIT_ERROR: u8 = 2;

/// Stack of the thread every command runs on.
///
/// The library's budgets (`max_block_nesting`, `max_text_box_nesting`,
/// `max_inline_nesting`, the math budgets) are sized so parsing fits the 1 MiB
/// main-thread stack Windows gives a process. This is defence in depth on top of
/// them: a recursion path a budget has not caught yet meets 64 MiB, not 1 MiB.
const WORKER_STACK: usize = 64 * 1024 * 1024;

fn main() -> ExitCode {
    let worker = std::thread::Builder::new()
        .name("strict-ooxml".to_owned())
        .stack_size(WORKER_STACK)
        .spawn(run_command);
    match worker {
        Ok(handle) => handle.join().unwrap_or_else(|_| {
            eprintln!("error: internal error (the command panicked)");
            ExitCode::from(EXIT_ERROR)
        }),
        Err(error) => {
            // A host that refuses a 64 MiB reservation still gets the command,
            // on the stack it already has.
            eprintln!(
                "warning: could not reserve a {WORKER_STACK}-byte stack ({error}); continuing"
            );
            run_command()
        }
    }
}

/// Dispatches the command line; runs on the [`WORKER_STACK`] thread.
fn run_command() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("inspect") => run_inspect(&args.collect::<Vec<_>>()),
        Some("check") => run_check(&args.collect::<Vec<_>>()),
        Some("report") => run_report(&args.collect::<Vec<_>>()),
        Some("render") => run_render(&args.collect::<Vec<_>>()),
        Some("to-pdf") => run_to_pdf(&args.collect::<Vec<_>>()),
        Some("from-pdf") => run_from_pdf(&args.collect::<Vec<_>>()),
        Some("write") => run_write(&args.collect::<Vec<_>>()),
        Some("normalize") => run_normalize(&args.collect::<Vec<_>>()),
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
        "usage: strict-ooxml <inspect|check|report|render|to-pdf|from-pdf|write|normalize> <file> \
         [--json|--text] [--out <path>] [--report-out <path>] [--pages 1-3] [--scale 96] \
         [--no-floating] [--no-math] [--transitional]"
    );
    eprintln!();
    eprintln!("  --transitional  normalize a Transitional package to Strict on the way in");
    eprintln!("  normalize       open a Transitional package and print the Loss Report");
    eprintln!("  to-pdf          render a .docx to PDF with embedded, selectable text (--out is required)");
    eprintln!("  from-pdf        convert a .pdf to a Strict .docx [--mode semantic|visual] [--no-images] [--report-out <json>] (--out is required)");
    eprintln!("  write           serialize the model back to a Strict .docx [--report-out <json>] (--out is required)");
}

/// Opens a package, normalizing it when `--transitional` is present.
///
/// Returns the options and a handle to the normalizer that was installed, so
/// the caller can read the loss report — the only record of what
/// normalization cost.
fn open_options(transitional: bool) -> (OpenOptions, Option<Arc<TransitionalNormalizer>>) {
    if !transitional {
        return (OpenOptions::default(), None);
    }
    let normalizer = Arc::new(TransitionalNormalizer::new());
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .shared_normalization(normalizer.clone());
    (options, Some(normalizer))
}

/// `normalize`: open a Transitional package through the pipeline and print
/// what changed and what it cost.
///
/// Writes no file. The loss report is the artefact that matters, and writing a
/// normalized package is a separate concern from opening one.
fn run_normalize(args: &[String]) -> ExitCode {
    let Some(file) = args.first() else {
        eprintln!("error: 'normalize' requires a package path");
        return ExitCode::from(EXIT_ERROR);
    };
    let (options, normalizer) = open_options(true);
    let Some(normalizer) = normalizer else {
        return ExitCode::from(EXIT_ERROR);
    };
    let outcome = Package::open_path(file, &options);
    // Read every part so the report covers the whole package rather than only
    // the parts the walk happened to touch.
    if let Ok(package) = &outcome {
        for part in package.parts() {
            if let Err(error) = package.read_part(&part.id) {
                eprintln!("warning: {}: {error}", part.id);
            }
        }
    }
    let report = normalizer.report();
    println!(
        "conformance: {:?}",
        outcome
            .as_ref()
            .map_or(Conformance::Unknown, Package::conformance)
    );
    print!("{report}");
    match outcome {
        Ok(_) => match report.verify_no_silent_loss() {
            Ok(()) => ExitCode::from(EXIT_OK),
            Err(reason) => {
                eprintln!("error: {reason}");
                ExitCode::from(EXIT_ERROR)
            }
        },
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(EXIT_ERROR)
        }
    }
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

/// Prints the support report and maps it to an exit code.
///
/// A blocker is an `unsupported` or `error` feature (ADR-0005); `partial` is a
/// warning and does not change the exit code. `ok_line` is what to print when
/// nothing blocks, so a caller can distinguish an input that was already
/// Strict from one that had to be normalized.
fn report_support(document: &StrictDocument, ok_line: &str) -> ExitCode {
    let report = document.support_report();
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
        println!("{ok_line}");
    } else {
        println!("strict: {} blocker(s) require attention", blockers.len());
    }
    println!("overall: {}", report.overall_status);
    let summary = report.summary;
    println!(
        "summary: supported={} partial={} unsupported={} ignored={} error={}",
        summary.supported, summary.partial, summary.unsupported, summary.ignored, summary.error,
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

fn run_check(args: &[String]) -> ExitCode {
    let Some(path) = args.first() else {
        eprintln!("error: 'check' requires a package path");
        return ExitCode::from(EXIT_ERROR);
    };
    let (options, normalizer) = open_options(args.iter().any(|arg| arg == "--transitional"));
    // AUD-23 / ADR-0016: `opc::policy::decide` is now the only place that
    // weighs conformance against policy; by the time `open_path` returns
    // `Ok`, the package was already accepted. There is nothing left for
    // `check` to branch on — only how to word a success that is already won.
    let code = match StrictDocument::open_path(path, &options) {
        Ok(document) => {
            let package = document.package();
            let ok_line = if package.was_normalized() {
                format!("ok: normalized from {:?}", package.conformance()).to_lowercase()
            } else {
                "ok: strict".to_owned()
            };
            report_support(&document, &ok_line)
        }
        Err(error @ StrictError::TransitionalNotSupported { .. }) => {
            println!("transitional: {error}");
            eprintln!("hint: pass --transitional to normalize it to Strict on the way in");
            ExitCode::from(EXIT_PROBLEM)
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(EXIT_ERROR)
        }
    };
    print_loss(normalizer.as_deref(), code)
}

fn print_loss(normalizer: Option<&TransitionalNormalizer>, code: ExitCode) -> ExitCode {
    let Some(normalizer) = normalizer else {
        return code;
    };
    let report = normalizer.report();
    if !report.is_noop() {
        eprint!("{report}");
    }
    match report.verify_no_silent_loss() {
        Ok(()) => code,
        Err(reason) => {
            eprintln!("error: {reason}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

fn run_report(args: &[String]) -> ExitCode {
    let mut file: Option<&str> = None;
    let mut text = false;
    let mut out: Option<&str> = None;
    let mut transitional = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" => text = false,
            "--text" => text = true,
            "--transitional" => transitional = true,
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

    let (options, normalizer) = open_options(transitional);
    let code = match StrictDocument::open_path(file, &options) {
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
            eprintln!("error: {error}");
            eprintln!("hint: pass --transitional to normalize it to Strict on the way in");
            ExitCode::from(EXIT_ERROR)
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(EXIT_ERROR)
        }
    };
    print_loss(normalizer.as_deref(), code)
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
    math: bool,
}

impl RenderArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut file: Option<String> = None;
        let mut out: Option<String> = None;
        let mut pages: Option<PageSelection> = None;
        let mut scale: Option<f64> = None;
        let mut floating = true;
        let mut math = true;
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--no-floating" => floating = false,
                "--no-math" => math = false,
                // Accepting the flag here rather than filtering it out of
                // `args` keeps one source of truth for what the flag means.
                "--transitional" => {}
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
            math,
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
    let (options, normalizer) = open_options(args.iter().any(|arg| arg == "--transitional"));
    let code = match StrictDocument::open_path(&parsed.file, &options) {
        Ok(document) => render_parsed(&document, &parsed),
        Err(error @ StrictError::TransitionalNotSupported { .. }) => {
            eprintln!("error: {error}");
            eprintln!("hint: pass --transitional to normalize it to Strict on the way in");
            ExitCode::from(EXIT_ERROR)
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(EXIT_ERROR)
        }
    };
    print_loss(normalizer.as_deref(), code)
}

/// `to-pdf`: render the document to a PDF (`STAGE-8-TASK.md` §4).
///
/// Exit codes follow `render`: `0` when the PDF is written and nothing was lost,
/// `1` when it was written but the report has losses, `2` on failure.
fn run_to_pdf(args: &[String]) -> ExitCode {
    let parsed = match WriteArgs::parse(args) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(EXIT_ERROR);
        }
    };
    let (options, normalizer) = open_options(parsed.transitional);
    let code = match StrictDocument::open_path(&parsed.file, &options) {
        Ok(document) => match document.render_pdf(&RenderOptions::default()) {
            Ok(output) => {
                if !output.report.is_clean() {
                    eprintln!("warning: the render is not lossless");
                    eprint!("{}", output.report);
                }
                let input = match std::fs::read(&parsed.file) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        eprintln!("error: cannot read {}: {error}", parsed.file);
                        return ExitCode::from(EXIT_ERROR);
                    }
                };
                let mut summary = normalization_summary(normalizer.as_deref());
                summary.extend_issues(output.report.losses().iter().map(|loss| PipelineIssue {
                    stage: PipelineStage::Render,
                    id: loss.id.to_owned(),
                    severity: loss.severity.to_string(),
                    part: None,
                    page: None,
                    location: None,
                    count: 1,
                    detail: loss.detail.clone(),
                }));
                match commit_result(
                    &parsed.out,
                    parsed.report_out.as_deref(),
                    &input,
                    &output.bytes,
                    &summary,
                ) {
                    Ok(code) => {
                        println!(
                            "wrote {} ({} page(s), {} bytes, {} embedded face(s))",
                            parsed.out,
                            output.page_count,
                            output.bytes.len(),
                            output.embedded_faces.len()
                        );
                        code
                    }
                    Err(()) => ExitCode::from(EXIT_ERROR),
                }
            }
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::from(EXIT_ERROR)
            }
        },
        Err(error @ StrictError::TransitionalNotSupported { .. }) => {
            eprintln!("error: {error}");
            eprintln!("hint: pass --transitional to normalize it to Strict on the way in");
            ExitCode::from(EXIT_ERROR)
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(EXIT_ERROR)
        }
    };
    print_loss(normalizer.as_deref(), code)
}

/// Parsed `write` command arguments.
struct WriteArgs {
    file: String,
    out: String,
    transitional: bool,
    report_out: Option<String>,
}

impl WriteArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut file: Option<String> = None;
        let mut out: Option<String> = None;
        let mut transitional = false;
        let mut report_out = None;
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--transitional" => transitional = true,
                "--out" => {
                    index += 1;
                    out = Some(args.get(index).ok_or("'--out' requires a path")?.clone());
                }
                "--report-out" => {
                    index += 1;
                    report_out = Some(
                        args.get(index)
                            .ok_or("'--report-out' requires a path")?
                            .clone(),
                    );
                }
                flag if flag.starts_with("--") => {
                    return Err(format!("unknown option '{flag}'"));
                }
                positional => {
                    if file.is_some() {
                        return Err("'write' accepts a single file".to_owned());
                    }
                    file = Some(positional.to_owned());
                }
            }
            index += 1;
        }
        Ok(Self {
            file: file.ok_or("'write' requires a package path")?,
            out: out.ok_or("'write' requires --out <path>")?,
            transitional,
            report_out,
        })
    }
}

/// `from-pdf`: convert a PDF into a Strict `.docx` (`STAGE-8-TASK.md` §5.1).
///
/// Exit codes: `0` when the document is written and nothing was lost, `1` when
/// it was written and the report has losses, `2` on failure. A lossy conversion
/// is still a usable file, so it is a warning; but the report is always printed,
/// because a conversion that quietly dropped a diagram is worse than one that
/// says it could not place it.
// The argument loop is the function: splitting it would move the option
// table away from the thing it parses.
#[allow(clippy::too_many_lines)]
fn run_from_pdf(args: &[String]) -> ExitCode {
    let mut file: Option<&str> = None;
    let mut out: Option<&str> = None;
    let mut report_out: Option<&str> = None;
    let mut mode = strict_ooxml::Mode::Semantic;
    let mut no_images = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--mode" => {
                index += 1;
                let Some(value) = args.get(index).map(String::as_str) else {
                    eprintln!("error: '--mode' requires semantic or visual");
                    return ExitCode::from(EXIT_ERROR);
                };
                mode = match value {
                    "semantic" => strict_ooxml::Mode::Semantic,
                    "visual" => strict_ooxml::Mode::Visual,
                    other => {
                        eprintln!("error: unknown mode '{other}'");
                        return ExitCode::from(EXIT_ERROR);
                    }
                };
            }
            "--no-images" => no_images = true,
            "--report-out" => {
                index += 1;
                report_out = Some(args.get(index).map_or("", String::as_str));
                if report_out == Some("") {
                    eprintln!("error: '--report-out' requires a path");
                    return ExitCode::from(EXIT_ERROR);
                }
            }
            "--out" => {
                index += 1;
                out = Some(args.get(index).map_or("", String::as_str));
                if out == Some("") {
                    eprintln!("error: '--out' requires a path");
                    return ExitCode::from(EXIT_ERROR);
                }
            }
            flag if flag.starts_with("--") => {
                eprintln!("error: unknown option '{flag}'");
                return ExitCode::from(EXIT_ERROR);
            }
            positional => {
                if file.is_some() {
                    eprintln!("error: 'from-pdf' accepts a single file");
                    return ExitCode::from(EXIT_ERROR);
                }
                file = Some(positional);
            }
        }
        index += 1;
    }
    let (Some(file), Some(out)) = (file, out) else {
        eprintln!("error: 'from-pdf' requires a PDF path and --out <file.docx>");
        return ExitCode::from(EXIT_ERROR);
    };

    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("error: cannot read {file}: {error}");
            return ExitCode::from(EXIT_ERROR);
        }
    };
    let mut reader =
        match strict_ooxml::PdfDocument::open(&bytes, strict_ooxml::PdfLimits::default()) {
            Ok(reader) => reader,
            Err(error) => {
                eprintln!("error: {error}");
                return ExitCode::from(EXIT_ERROR);
            }
        };
    let options = strict_ooxml::PdfOptions::default()
        .mode(mode)
        .embed_images(!no_images);
    let converted = match strict_ooxml::convert_pdf(&mut reader, &options) {
        Ok(converted) => converted,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(EXIT_ERROR);
        }
    };
    print!("{}", converted.report);

    // The document references its images by part id; the bytes came out of the
    // PDF, so they travel with it.
    let mut bag = strict_ooxml::MediaBag::new();
    for (part, bytes) in &converted.media {
        bag.insert(part.clone(), bytes.clone());
    }
    let written = match strict_ooxml::write_package(
        &converted.document,
        Some(&bag),
        &strict_ooxml::WriteOptions::default(),
    ) {
        Ok(written) => written,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(EXIT_ERROR);
        }
    };
    let write_losses = written.report.losses();
    eprintln!("write: {} loss(es)", write_losses.len());
    for loss in &write_losses {
        eprintln!("  {loss}");
    }
    let mut summary = PipelineSummary::from_normalization(PipelineStage::Write, &written.report);
    summary.extend_issues(converted.report.pipeline_issues());
    if summary.outcome == PipelineOutcome::Degraded {
        eprintln!("warning: the conversion is not lossless");
    }
    match commit_result(out, report_out, &bytes, &written.bytes, &summary) {
        Ok(code) => {
            println!(
                "wrote {out} ({} blocks, {} bytes)",
                converted.document.body.blocks.len(),
                written.bytes.len()
            );
            code
        }
        Err(()) => ExitCode::from(EXIT_ERROR),
    }
}

/// `write`: serialize the parsed model back to a Strict package (Stage 8A).
///
/// The exit code follows the `render` convention: `0` when the package was
/// written and the writer dropped nothing that reaches the page, `1` when it
/// was written but something was lost, `2` on failure. A lossy write is still a
/// usable file, so it is a warning rather than an error — but never a silent
/// one, which is why the report is printed unconditionally.
fn run_write(args: &[String]) -> ExitCode {
    let parsed = match WriteArgs::parse(args) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(EXIT_ERROR);
        }
    };
    let (options, normalizer) = open_options(parsed.transitional);
    let package = match Package::open_path(&parsed.file, &options) {
        Ok(package) => package,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(EXIT_ERROR);
        }
    };
    let document = match StrictDocument::from_package(package, &strict_ooxml::WmlOptions::default())
    {
        Ok(document) => document,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(EXIT_ERROR);
        }
    };

    let written = match strict_ooxml::write_package(
        document.document(),
        Some(document.package()),
        &strict_ooxml::WriteOptions::default(),
    ) {
        Ok(written) => written,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(EXIT_ERROR);
        }
    };
    let input = match std::fs::read(&parsed.file) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("error: cannot read {}: {error}", parsed.file);
            return ExitCode::from(EXIT_ERROR);
        }
    };
    let dropped = strict_ooxml::dropped_count(&written.report);
    if dropped > 0 {
        eprintln!("warning: {dropped} construct(s) could not be written");
    }
    for loss in written.report.losses() {
        eprintln!("  {loss}");
    }
    // Parser feature ids have to be in this process's own report. A registry
    // entry named `named_loss` is not a report; the census only accepts the
    // loss when the feature id is printed for this input.
    for feature in document.support().iter() {
        if feature.status == strict_ooxml::model::support::SupportStatus::Supported {
            continue;
        }
        eprintln!(
            "support: [{}] {} x{} {}",
            feature.status.as_str(),
            feature.feature_id,
            feature.count,
            feature.message.as_deref().unwrap_or("")
        );
    }
    let summary = normalization_summary(normalizer.as_deref()).merge(
        PipelineSummary::from_normalization(PipelineStage::Write, &written.report),
    );
    let code = match commit_result(
        &parsed.out,
        parsed.report_out.as_deref(),
        &input,
        &written.bytes,
        &summary,
    ) {
        Ok(code) => {
            println!(
                "wrote {} ({} part(s), {} bytes)",
                parsed.out,
                written.part_count,
                written.bytes.len()
            );
            code
        }
        Err(()) => ExitCode::from(EXIT_ERROR),
    };
    print_loss(normalizer.as_deref(), code)
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
    render_options = render_options.math(parsed.math);
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

fn normalization_summary(normalizer: Option<&TransitionalNormalizer>) -> PipelineSummary {
    normalizer.map_or_else(PipelineSummary::new, |normalizer| {
        let report = normalizer.report();
        PipelineSummary::from_normalization(PipelineStage::Normalize, &report)
    })
}

fn print_pipeline(summary: &PipelineSummary) {
    for issue in &summary.issues {
        eprintln!(
            "stage {}: [{}] {} x{} {}",
            issue.stage, issue.severity, issue.id, issue.count, issue.detail
        );
    }
}

fn exit_of(summary: &PipelineSummary) -> ExitCode {
    ExitCode::from(match summary.outcome {
        PipelineOutcome::Clean => EXIT_OK,
        PipelineOutcome::Degraded => EXIT_PROBLEM,
        PipelineOutcome::Failed => EXIT_ERROR,
    })
}

/// Writes `output` and, when requested, the sidecar. `Err` means a requested
/// file was not stored; the partial name is not the result.
fn commit_result(
    out: &str,
    report_out: Option<&str>,
    input: &[u8],
    output: &[u8],
    summary: &PipelineSummary,
) -> Result<ExitCode, ()> {
    print_pipeline(summary);
    if let Err(error) = atomic_write(Path::new(out), output) {
        eprintln!("error: cannot write {out}: {error}");
        return Err(());
    }
    if let Some(path) = report_out {
        let json = sidecar_json(input, output, summary);
        if let Err(error) = atomic_write(Path::new(path), json.as_bytes()) {
            eprintln!("error: cannot write report {path}: {error}");
            return Err(());
        }
    }
    Ok(exit_of(summary))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut temporary_name = path.as_os_str().to_owned();
    temporary_name.push(".partial");
    let temporary = PathBuf::from(temporary_name);
    std::fs::write(&temporary, bytes)?;
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn json_escape(text: &str) -> String {
    let mut escaped = String::new();
    for character in text.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other if other.is_control() => {
                let _ = write!(escaped, "\\u{value:04x}", value = u32::from(other));
            }
            other => escaped.push(other),
        }
    }
    escaped
}

fn sidecar_json(input: &[u8], output: &[u8], summary: &PipelineSummary) -> String {
    let mut body = String::new();
    let _ = writeln!(body, "{{");
    let _ = writeln!(body, "  \"version\": 1,");
    let _ = writeln!(body, "  \"input_sha256\": \"{}\",", sha256_hex(input));
    let _ = writeln!(body, "  \"output_sha256\": \"{}\",", sha256_hex(output));
    let _ = writeln!(body, "  \"outcome\": \"{}\",", summary.outcome);
    let _ = writeln!(body, "  \"issues\": [");
    for (index, issue) in summary.issues.iter().enumerate() {
        let _ = writeln!(body, "    {{");
        push_json_string(&mut body, "stage", &issue.stage.to_string(), true);
        push_json_string(&mut body, "id", &issue.id, true);
        push_json_string(&mut body, "severity", &issue.severity, true);
        if let Some(part) = &issue.part {
            push_json_string(&mut body, "part", part, true);
        }
        if let Some(page) = issue.page {
            let _ = writeln!(body, "      \"page\": {page},");
        }
        if let Some(location) = &issue.location {
            push_json_string(&mut body, "location", location, true);
        }
        let _ = writeln!(body, "      \"count\": {},", issue.count);
        push_json_string(&mut body, "detail", &issue.detail, false);
        body.push_str("    }");
        if index + 1 != summary.issues.len() {
            body.push(',');
        }
        body.push('\n');
    }
    let _ = writeln!(body, "  ]");
    let _ = writeln!(body, "}}");
    body
}

fn push_json_string(body: &mut String, name: &str, value: &str, comma: bool) {
    let _ = write!(body, "      \"{name}\": \"{}\"", json_escape(value));
    if comma {
        body.push(',');
    }
    body.push('\n');
}

/// Replaces C0/C1 control characters other than newline and tab with a visible
/// `\u{..}` escape, so text taken from a document cannot drive the terminal.
///
/// JSON output passes through unchanged: the serializer already escapes control
/// characters inside strings, and nothing else in it is a control character.
fn terminal_safe(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.chars().any(is_unsafe_control) {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        if is_unsafe_control(ch) {
            let _ = write!(out, "\\u{{{:x}}}", u32::from(ch));
        } else {
            out.push(ch);
        }
    }
    std::borrow::Cow::Owned(out)
}

fn is_unsafe_control(ch: char) -> bool {
    ch.is_control() && ch != '\n' && ch != '\t'
}
