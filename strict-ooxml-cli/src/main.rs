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

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use strict_ooxml::{
    ConformancePolicy, Feature, FeatureStatus, Location, PageSelection, RenderOptions,
    StrictDocument, TransitionalNormalizer,
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
        "usage: strict-ooxml <inspect|check|report|render|write|normalize> <file.docx> \
         [--json|--text] [--out <path>] [--pages 1-3] [--scale 96] [--no-floating] [--no-math] \
         [--transitional]"
    );
    eprintln!();
    eprintln!("  --transitional  normalize a Transitional package to Strict on the way in");
    eprintln!("  normalize       open a Transitional package and print the Loss Report");
    eprintln!("  write           serialize the model back to a Strict .docx (--out is required)");
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
    let code = match StrictDocument::open_path(path, &options) {
        // `Mixed` and `Unknown` are the expected *starting* state under
        // normalization, not a verdict: they are what stage T0 sees before any
        // part is rewritten. Reporting them as errors would make
        // `--transitional` useless on most real documents. Without a
        // normalizer they stay errors — a package whose conformance cannot be
        // determined is not something to report on as strict.
        Ok(document)
            if matches!(
                document.package().conformance(),
                Conformance::Strict | Conformance::Transitional
            ) || (normalizer.is_some()
                && matches!(
                    document.package().conformance(),
                    Conformance::Mixed | Conformance::Unknown
                )) =>
        {
            report_support(&document, "ok: strict")
        }
        Ok(document) => {
            match document.package().conformance() {
                Conformance::Mixed => {
                    println!("mixed: the package carries Strict and Transitional signals");
                }
                _ => println!("unknown: conformance could not be determined"),
            }
            ExitCode::from(EXIT_ERROR)
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

/// Parsed `write` command arguments.
struct WriteArgs {
    file: String,
    out: String,
    transitional: bool,
}

impl WriteArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut file: Option<String> = None;
        let mut out: Option<String> = None;
        let mut transitional = false;
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--transitional" => transitional = true,
                "--out" => {
                    index += 1;
                    out = Some(args.get(index).ok_or("'--out' requires a path")?.clone());
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
        })
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
    if let Err(error) = std::fs::write(&parsed.out, &written.bytes) {
        eprintln!("error: cannot write {}: {error}", parsed.out);
        return ExitCode::from(EXIT_ERROR);
    }

    println!(
        "wrote {} ({} part(s), {} bytes)",
        parsed.out,
        written.part_count,
        written.bytes.len()
    );
    let dropped = strict_ooxml::dropped_count(&written.report);
    if dropped > 0 {
        eprintln!("warning: {dropped} construct(s) could not be written");
    }
    for loss in written.report.losses() {
        eprintln!("  {loss}");
    }
    let code = match strict_ooxml::verify_no_silent_loss(&written.report) {
        Ok(()) if dropped == 0 => ExitCode::from(EXIT_OK),
        Ok(()) => ExitCode::from(EXIT_PROBLEM),
        Err(reason) => {
            eprintln!("error: {reason}");
            ExitCode::from(EXIT_ERROR)
        }
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
