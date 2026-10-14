//! A local viewer for rendered `WordprocessingML` documents.
//!
//! Scans a directory of `.docx` files, renders whichever one is chosen, and
//! serves the pages as SVG in a scrolling column with the page boundaries
//! visible. Meant for judging a render — whether a glyph landed in the right
//! place — which is why it serves vector pages instead of rasterizing them.
//!
//! ```text
//! strict-ooxml-view [DIR...] [--port N] [--transitional] [--scale N] [--cell-nowrap=spec|always]
//! ```
//!
//! With no directory it serves every known corpus that contains documents, and
//! the page can switch between them. Directories on the command line replace
//! that list.
// Never-crash (docs/WORDCRAFT_ADOPTION_2026-10-07.md §4.2): library paths
// return errors or degrade with a report; tests may still unwrap.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::indexing_slicing
    )
)]

mod http;
mod process_limit;
mod ui;
mod worker;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
#[cfg(test)]
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use http::Response;
#[cfg(test)]
use strict_ooxml::OpenControl;
use strict_ooxml_view::{discover, Cache, DocumentView, Entry, RenderSettings};
#[cfg(test)]
use strict_ooxml_view::{failed, render_controlled};

/// Exit code: the viewer ran and stopped cleanly.
const EXIT_OK: u8 = 0;
/// Exit code: the viewer could not start.
const EXIT_ERROR: u8 = 2;

/// How long one document may take to render before its request is answered
/// with a failed view.
///
/// The server is single-threaded, so a render that does not finish is a viewer
/// that does not answer — not for that document, for anything. Past this the
/// request gets a view whose note says so, and the document is cached as that
/// failure: asking again does not start a second render of a document that
/// already showed it will not finish.
const RENDER_TIMEOUT: Duration = Duration::from_secs(30);

/// Stack for the render thread.
///
/// The renderer is budgeted for a 1 MiB stack (the Windows main thread), and a
/// spawned thread gets 2 MiB by default; the Linux main thread the render used
/// to run on has 8. The same 8 here, so moving the render off the main thread
/// costs no document its render.
#[cfg(test)]
const RENDER_STACK_BYTES: usize = 8 * 1024 * 1024;

/// One directory the viewer offers in the corpus menu.
struct KnownCorpus {
    /// Stable id used in URLs.
    id: &'static str,
    /// Label shown in the menu.
    label: &'static str,
    /// Path relative to the workspace root.
    path: &'static str,
    /// Whether packages in this corpus are Transitional and must be normalized.
    transitional: bool,
}

/// Corpora offered when the command line names none.
const KNOWN_CORPORA: &[KnownCorpus] = &[
    KnownCorpus {
        id: "strict",
        label: "Strict",
        path: "strict-ooxml-core/tests/strict",
        transitional: false,
    },
    KnownCorpus {
        id: "docx",
        label: "Docx",
        path: "strict-ooxml-core/tests/docx",
        transitional: true,
    },
    KnownCorpus {
        id: "cc0-docx",
        label: "CC0 DOCX",
        path: "testdata/CC0_DOCX",
        transitional: true,
    },
    KnownCorpus {
        id: "cc0-docx-math",
        label: "CC0 DOCX Math",
        path: "testdata/CC0_DOCX_MATH",
        transitional: true,
    },
    KnownCorpus {
        id: "cc0",
        label: "CC0",
        path: "testdata/CC0",
        transitional: true,
    },
    KnownCorpus {
        id: "cc0-rtf",
        label: "CC0 RTF",
        path: "testdata/CC0_RTF",
        transitional: false,
    },
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--render-worker") {
        let Some(path) = args.get(1) else {
            return ExitCode::from(EXIT_ERROR);
        };
        return match worker::entry(Path::new(path)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::from(EXIT_ERROR)
            }
        };
    }
    let Some(config) = Config::parse(&args) else {
        return ExitCode::from(EXIT_ERROR);
    };

    let corpora = load_corpora(&config);
    if corpora.is_empty() {
        eprintln!("error: no .docx files found");
        eprintln!("hint: pass one or more directories, e.g. strict-ooxml-view testdata/CC0_DOCX");
        return ExitCode::from(EXIT_ERROR);
    }

    println!("serving {} corpus(es)", corpora.len());
    for corpus in &corpora {
        let mode = if corpus.transitional {
            "normalized"
        } else {
            "strict"
        };
        println!(
            "  {} — {} document(s), {mode}, {}",
            corpus.label,
            corpus.entries.len(),
            corpus.path
        );
    }

    let state = Arc::new(State {
        corpora,
        caches: Mutex::new(BTreeMap::new()),
        settings: RenderSettings {
            scale: config.scale,
            cell_no_wrap: config.cell_no_wrap,
        },
        rtf: config.rtf,
    });

    let listener = match std::net::TcpListener::bind(("127.0.0.1", config.port)) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("error: cannot bind 127.0.0.1:{}: {error}", config.port);
            return ExitCode::from(EXIT_ERROR);
        }
    };
    let port = listener
        .local_addr()
        .map_or(config.port, |addr| addr.port());
    println!("open http://127.0.0.1:{port}/");
    let _ = std::io::Write::flush(&mut std::io::stdout());

    let stop = Arc::new(AtomicBool::new(false));
    let handler_state = Arc::clone(&state);
    if let Err(error) = http::serve(&listener, &stop, move |path, query| {
        route(&handler_state, path, query)
    }) {
        eprintln!("error: {error}");
        return ExitCode::from(EXIT_ERROR);
    }
    ExitCode::from(EXIT_OK)
}

/// One corpus the page can switch to.
struct Corpus {
    /// Stable id used in URLs.
    id: String,
    /// Label shown in the menu.
    label: String,
    /// Directory, as printed at startup.
    ///
    /// Not returned by the API: the page never used it, and an absolute path
    /// is the user's home directory and account name handed to any script
    /// that can reach the port.
    path: String,
    /// Documents in menu order.
    entries: Vec<Entry>,
    /// Whether to normalize Transitional packages in this corpus.
    transitional: bool,
}

/// Everything the routes need.
struct State {
    /// Corpora, in menu order.
    corpora: Vec<Corpus>,
    /// Rendered pages, one cache per corpus so equal file names do not collide.
    caches: Mutex<BTreeMap<String, Cache>>,
    /// Explicit RTF compatibility policies, ignored for DOCX entries.
    rtf: worker::RtfFlags,
    /// Output scale (px per inch) and `w:noWrap` handling.
    ///
    /// The scale is a DPI, not a multiplier: the renderer converts twips by
    /// dividing by 1440 and multiplying by this. Passing 1.0 renders an
    /// 8.5x11 inch page as 8.5x11 *pixels* — a postage stamp — and the
    /// content then overflows into a dozen pages.
    settings: RenderSettings,
}

impl State {
    /// Returns the view for `name` in `corpus`, rendering it on first request.
    fn view(&self, corpus: &Corpus, name: &str) -> Option<Arc<DocumentView>> {
        if let Some(cached) = self
            .caches
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&corpus.id)
            .and_then(|cache| cache.get_shared(name))
        {
            return Some(cached);
        }
        let entry = corpus.entries.iter().find(|entry| entry.name == name)?;
        let view = Arc::new(render_bounded(
            entry,
            corpus.transitional,
            self.settings,
            self.rtf,
            RENDER_TIMEOUT,
        ));
        self.caches
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(corpus.id.clone())
            .or_default()
            .insert_shared(Arc::clone(&view));
        Some(view)
    }
}

/// Why [`run_bounded`] came back without a value.
#[derive(Debug, PartialEq, Eq)]
#[cfg(test)]
enum Unfinished {
    /// The job is still running; it was left to finish on its own.
    TimedOut,
    /// The job ended without an answer (it panicked), or never started.
    Lost(String),
}

/// Runs `job` on its own thread and waits for it at most `timeout`.
///
/// A job that does not finish is not stopped — a thread cannot be — but the
/// caller stops waiting for it, and its answer, if it ever comes, goes nowhere.
#[cfg(test)]
fn run_bounded<T, F>(timeout: Duration, job: F) -> Result<T, Unfinished>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("render".to_owned())
        .stack_size(RENDER_STACK_BYTES)
        .spawn(move || {
            // The receiver is gone when the caller has stopped waiting.
            let _ = sender.send(job());
        });
    if let Err(error) = spawned {
        return Err(Unfinished::Lost(format!(
            "could not start a thread: {error}"
        )));
    }
    match receiver.recv_timeout(timeout) {
        Ok(value) => Ok(value),
        Err(RecvTimeoutError::Timeout) => Err(Unfinished::TimedOut),
        Err(RecvTimeoutError::Disconnected) => Err(Unfinished::Lost(
            "the renderer stopped without an answer".to_owned(),
        )),
    }
}

/// Renders `entry` off the server thread, giving up after `timeout`.
///
/// Always a view: a document that will not open, will not finish, or takes the
/// renderer down with it is still a menu entry that has to say so; showing
/// nothing would look like a viewer bug.
fn render_bounded(
    entry: &Entry,
    transitional: bool,
    settings: RenderSettings,
    rtf: worker::RtfFlags,
    timeout: Duration,
) -> DocumentView {
    worker::run(entry, transitional, settings, rtf, timeout)
}

#[cfg(test)]
fn render_in_test_thread(
    entry: &Entry,
    transitional: bool,
    settings: RenderSettings,
    timeout: Duration,
) -> DocumentView {
    let job = entry.clone();
    let control = OpenControl::with_timeout(timeout);
    let worker_control = control.clone();
    let outcome = run_bounded(timeout, move || {
        render_controlled(&job, transitional, settings, &worker_control)
            .map_err(|error| error.to_string())
    });
    if matches!(outcome, Err(Unfinished::TimedOut)) {
        control.cancel();
    }
    match outcome {
        Ok(Ok(view)) => view,
        Ok(Err(reason)) | Err(Unfinished::Lost(reason)) => failed(entry, &reason),
        Err(Unfinished::TimedOut) => failed(
            entry,
            &format!(
                "rendering did not finish within {} s; cancellation requested",
                timeout.as_secs()
            ),
        ),
    }
}

/// Answers one request.
fn route(state: &State, path: &str, query: &str) -> Response {
    match path {
        "/" | "/index.html" => Response::ok("text/html; charset=utf-8", ui::page().into_bytes()),
        "/api/corpora" => Response::ok(
            "application/json; charset=utf-8",
            format!(
                "[{}]",
                state
                    .corpora
                    .iter()
                    .map(corpus_json)
                    .collect::<Vec<_>>()
                    .join(",")
            )
            .into_bytes(),
        ),
        "/api/documents" => {
            let Some(corpus) = selected_corpus(state, query) else {
                return Response::not_found("corpus");
            };
            let listing: Vec<String> = corpus
                .entries
                .iter()
                .map(|entry| menu_entry_json(&entry.name, entry.size))
                .collect();
            Response::ok(
                "application/json; charset=utf-8",
                format!("[{}]", listing.join(",")).into_bytes(),
            )
        }
        "/api/document" => {
            let Some(corpus) = selected_corpus(state, query) else {
                return Response::not_found("corpus");
            };
            let name = query_value(query, "name").unwrap_or_default();
            if name.is_empty() {
                return Response::bad_request("`name` is required");
            }
            let Some(view) = state.view(corpus, &name) else {
                return Response::not_found(&name);
            };
            let metadata_only = query_value(query, "metadata").as_deref() == Some("1");
            if !metadata_only && view.svg_bytes() > 96 * 1024 * 1024 {
                return Response::payload_too_large();
            }
            let body = match document_json_mode(&view, metadata_only) {
                Ok(json) => json.into_bytes(),
                Err(error) => {
                    return Response::bad_request(&format!("stored page unavailable: {error}"))
                }
            };
            worker::log(
                &serde_json::json!({"event":"response","document":name,"corpus":corpus.id,"bytes":body.len(),"metadata_only":metadata_only,"extreme":body.len()>8*1024*1024}),
            );
            Response::ok("application/json; charset=utf-8", body)
        }
        "/api/page" => {
            let Some(corpus) = selected_corpus(state, query) else {
                return Response::not_found("corpus");
            };
            let name = query_value(query, "name").unwrap_or_default();
            let Some(number) =
                query_value(query, "page").and_then(|value| value.parse::<usize>().ok())
            else {
                return Response::bad_request("page is required");
            };
            let Some(view) = state.view(corpus, &name) else {
                return Response::not_found(&name);
            };
            let Some(page) = view.pages.iter().find(|page| page.number == number) else {
                return Response::not_found("page");
            };
            match view.page_svg(page) {
                Ok(svg) => Response::ok("image/svg+xml; charset=utf-8", svg.into_bytes()),
                Err(error) => Response::bad_request(&format!("stored page unavailable: {error}")),
            }
        }
        other => Response::not_found(other),
    }
}

/// The corpus named by `corpus`, or the first one when the query omits it.
fn selected_corpus<'a>(state: &'a State, query: &str) -> Option<&'a Corpus> {
    let requested = query_value(query, "corpus").unwrap_or_default();
    if requested.is_empty() {
        return state.corpora.first();
    }
    state.corpora.iter().find(|corpus| corpus.id == requested)
}

/// One corpus as JSON.
fn corpus_json(corpus: &Corpus) -> String {
    format!(
        "{{\"id\":{},\"label\":{},\"count\":{},\"transitional\":{}}}",
        json_string(&corpus.id),
        json_string(&corpus.label),
        corpus.entries.len(),
        corpus.transitional
    )
}

/// One menu entry as JSON.
fn menu_entry_json(name: &str, size: u64) -> String {
    format!("{{\"name\":{},\"size\":{size}}}", json_string(name))
}

/// A rendered view as the JSON the page script expects.
#[cfg(test)]
fn document_json(view: &DocumentView) -> String {
    document_json_mode(view, false).unwrap_or_default()
}

fn document_json_mode(view: &DocumentView, metadata_only: bool) -> std::io::Result<String> {
    let mut pages = String::new();
    for page in &view.pages {
        if !pages.is_empty() {
            pages.push(',');
        }
        let _ = write!(
            pages,
            "{{\"number\":{},\"width\":{},\"height\":{}",
            page.number,
            json_number(page.width),
            json_number(page.height)
        );
        if !metadata_only {
            pages.push_str(",\"svg\":");
            write_json_string(&mut pages, &view.page_svg(page)?);
        }
        pages.push('}');
    }
    // An absent value must serialize as JSON `null`, not as nothing: writing
    // an empty string where a value is expected produces `"note":,` and the
    // client cannot parse the document at all.
    let summary = view.summary.map_or_else(
        || "null".to_owned(),
        |summary| {
            format!(
            "{{\"supported\":{},\"partial\":{},\"unsupported\":{},\"ignored\":{},\"error\":{}}}",
            summary.supported,
            summary.partial,
            summary.unsupported,
            summary.ignored,
            summary.error
        )
        },
    );
    let note = view
        .note
        .as_deref()
        .map_or_else(|| "null".to_owned(), json_string);
    Ok(format!(
        "{{\"name\":{},\"conformance\":{},\"summary\":{summary},\"note\":{note},\"pipeline\":{},\"pages\":[{}]}}",
        json_string(&view.name),
        json_string(&view.conformance),
        pipeline_json(&view.pipeline),
        pages
    ))
}

/// The loss ledger, kept apart from the mechanism summary.
fn pipeline_json(pipeline: &strict_ooxml_view::PipelineView) -> String {
    let issues = pipeline
        .issues
        .iter()
        .map(issue_json)
        .collect::<Vec<_>>()
        .join(",");
    let stages = pipeline
        .stages
        .iter()
        .map(|stage| {
            format!(
                "{{\"stage\":{},\"status\":{}}}",
                json_string(&stage.stage),
                json_string(&stage.status)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"outcome\":{},\"sidecar\":{},\"issues\":[{issues}],\"stages\":[{stages}]}}",
        json_string(&pipeline.outcome),
        json_string(&pipeline.sidecar)
    )
}

fn issue_json(issue: &strict_ooxml_view::ViewIssue) -> String {
    let mut body = format!(
        "{{\"stage\":{},\"id\":{},\"severity\":{},\"count\":{},\"detail\":{}",
        json_string(&issue.stage),
        json_string(&issue.id),
        json_string(&issue.severity),
        issue.count,
        json_string(&issue.detail)
    );
    if let Some(part) = &issue.part {
        let _ = write!(body, ",\"part\":{}", json_string(part));
    }
    if let Some(page) = issue.page {
        let _ = write!(body, ",\"page\":{page}");
    }
    if let Some(location) = &issue.location {
        let _ = write!(body, ",\"location\":{}", json_string(location));
    }
    body.push('}');
    body
}

/// Escapes `value` as a JSON string, quotes included.
///
/// The only place escaping happens, so no caller can forget it and emit a
/// page of valid-but-unparseable JSON.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    write_json_string(&mut out, value);
    out
}

fn write_json_string(out: &mut String, value: &str) {
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Control characters have to be escaped or the JSON is invalid.
            other if (other as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", other as u32);
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// Formats a number, or `0` when it is not finite.
///
/// A non-finite page width would reach the layout as `NaN` and collapse the
/// column, so it is pinned here rather than trusted.
fn json_number(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_owned();
    }
    let text = format!("{value:.2}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

/// Extracts one query parameter, percent-decoded.
fn query_value(query: &str, key: &str) -> Option<String> {
    for pair in query.split('&') {
        if let Some((name, value)) = pair.split_once('=') {
            if name == key {
                return http::percent_decode(value);
            }
        }
    }
    None
}

/// Parsed command line.
struct Config {
    /// Directories given on the command line, in order.
    dirs: Vec<PathBuf>,
    /// Explicit RTF compatibility policies.
    rtf: worker::RtfFlags,
    /// Port to bind.
    port: u16,
    /// Whether to normalize every corpus, including Strict.
    transitional: bool,
    /// Render scale.
    scale: f64,
    /// `--cell-nowrap=spec|always`.
    cell_no_wrap: strict_ooxml::CellNoWrap,
}

impl Config {
    /// Parses `args`, or returns `None` after printing what was wrong.
    fn parse(args: &[String]) -> Option<Self> {
        let mut config = Self {
            dirs: Vec::new(),
            rtf: worker::RtfFlags::default(),
            port: 8181,
            transitional: false,
            scale: 96.0,
            cell_no_wrap: strict_ooxml::CellNoWrap::default(),
        };
        let mut index = 0;
        while let Some(arg) = args.get(index) {
            match arg.as_str() {
                "--port" => {
                    index += 1;
                    let Some(value) = args.get(index) else {
                        eprintln!("error: --port needs a number from 1 to 65535");
                        return None;
                    };
                    let Some(port) = value.parse().ok().filter(|port: &u16| *port > 0) else {
                        eprintln!("error: --port needs a number from 1 to 65535");
                        return None;
                    };
                    config.port = port;
                }
                "--scale" => {
                    index += 1;
                    let value = args.get(index)?;
                    config.scale = value
                        .parse()
                        .ok()
                        .filter(|scale: &f64| scale.is_finite() && *scale > 0.0)?;
                }
                "--transitional" => config.transitional = true,
                other if other.starts_with("--cell-nowrap=") => {
                    let Some(mode) = other
                        .strip_prefix("--cell-nowrap=")
                        .and_then(strict_ooxml::CellNoWrap::parse)
                    else {
                        eprintln!("error: --cell-nowrap expects 'spec' or 'always'");
                        return None;
                    };
                    config.cell_no_wrap = mode;
                }
                "--wps-text" => {
                    config.rtf.wps_text = true;
                }
                "--allow-unsigned-unicode" => {
                    config.rtf.allow_unsigned_unicode = true;
                }
                "--drop-nul" => {
                    config.rtf.drop_nul = true;
                }
                "--reject-unsupported" => {
                    config.rtf.reject_unsupported = true;
                }
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                other if other.starts_with('-') => {
                    eprintln!("error: unknown option '{other}'");
                    print_usage();
                    return None;
                }
                other => config.dirs.push(PathBuf::from(other)),
            }
            index += 1;
        }
        Some(config)
    }
}

/// Corpora to serve: the directories on the command line, or every known
/// corpus that contains a document.
fn load_corpora(config: &Config) -> Vec<Corpus> {
    let requested = if config.dirs.is_empty() {
        KNOWN_CORPORA
            .iter()
            .map(|known| PathBuf::from(known.path))
            .collect()
    } else {
        config.dirs.clone()
    };
    let mut corpora = Vec::new();
    let mut used = BTreeSet::new();
    for dir in requested {
        let entries = discover(&dir);
        if entries.is_empty() {
            continue;
        }
        let known = known_for(&dir);
        let transitional = config.transitional || known.is_some_and(|item| item.transitional);
        let base_id = known.map_or_else(|| slug(&dir_label(&dir)), |item| item.id.to_owned());
        let label = known.map_or_else(|| dir_label(&dir), |item| item.label.to_owned());
        corpora.push(Corpus {
            id: unique_id(&base_id, &mut used),
            label,
            path: dir.display().to_string(),
            entries,
            transitional,
        });
    }
    corpora
}

/// The known corpus `dir` points at, when it is one of [`KNOWN_CORPORA`].
fn known_for(dir: &Path) -> Option<&'static KnownCorpus> {
    KNOWN_CORPORA
        .iter()
        .find(|known| same_dir(Path::new(known.path), dir))
}

/// Whether `left` and `right` name the same directory.
fn same_dir(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

/// The directory's own name, used when it is not a known corpus.
fn dir_label(dir: &Path) -> String {
    dir.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| dir.display().to_string())
}

/// A URL-safe id derived from a directory name.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            out.push(character.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "corpus".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// `base`, or `base-2`, `base-3`, … when `base` is already taken.
fn unique_id(base: &str, used: &mut BTreeSet<String>) -> String {
    if used.insert(base.to_owned()) {
        return base.to_owned();
    }
    let mut index = 2u32;
    loop {
        let candidate = format!("{base}-{index}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        index += 1;
    }
}

fn print_usage() {
    println!(
        "usage: strict-ooxml-view [DIR...] [--port N] [--scale N] [--transitional] [--cell-nowrap=spec|always]"
    );
    println!();
    println!("  DIR...            directories of .docx/.rtf files");
    println!("                    (default: every known corpus that contains documents)");
    println!("  --wps-text --allow-unsigned-unicode --drop-nul --reject-unsupported");
    println!("                    explicit RTF compatibility policies");
    println!("  --port N          port to bind on 127.0.0.1 (default 8181)");
    println!("  --scale N         output DPI (default 96; 144 doubles the page size)");
    println!("  --cell-nowrap=M   w:noWrap cells: spec (default, as ISO 29500) or always");
    println!("  --transitional    normalize every corpus, including Strict");
    println!();
    println!("known corpora:");
    for known in KNOWN_CORPORA {
        let mode = if known.transitional {
            "normalized"
        } else {
            "strict"
        };
        println!("  {:<12} {} ({mode})", known.label, known.path);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        corpus_json, json_number, json_string, known_for, query_value,
        render_in_test_thread as render_bounded, run_bounded, slug, unique_id, Config, Corpus,
        Entry, Unfinished, EXIT_OK, KNOWN_CORPORA,
    };
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::time::Duration;
    use strict_ooxml_view::{DocumentView, PipelineView};

    fn empty_pipeline() -> PipelineView {
        PipelineView {
            outcome: "clean".to_owned(),
            issues: Vec::new(),
            stages: Vec::new(),
            sidecar: "absent".to_owned(),
        }
    }

    #[test]
    fn defaults_are_sensible() {
        let config = Config::parse(&[]).expect("parse");
        assert_eq!(config.port, 8181);
        assert!(!config.transitional);
        assert!((config.scale - 96.0).abs() < f64::EPSILON);
        assert!(config.dirs.is_empty());
    }

    #[test]
    fn options_are_parsed() {
        let args: Vec<String> = [
            "corpus",
            "other",
            "--port",
            "9000",
            "--scale",
            "144",
            "--transitional",
        ]
        .iter()
        .map(|value| (*value).to_owned())
        .collect();
        let config = Config::parse(&args).expect("parse");
        assert_eq!(
            config.dirs,
            vec![PathBuf::from("corpus"), PathBuf::from("other")]
        );
        assert_eq!(config.port, 9000);
        assert!((config.scale - 144.0).abs() < f64::EPSILON);
        assert!(config.transitional);
    }

    #[test]
    fn a_bad_port_or_scale_is_rejected() {
        for bad in [
            vec!["--port", "0"],
            vec!["--port", "not-a-number"],
            vec!["--scale", "0"],
            vec!["--scale", "-1"],
        ] {
            let args: Vec<String> = bad.iter().map(|value| (*value).to_owned()).collect();
            assert!(Config::parse(&args).is_none(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn an_unknown_option_is_rejected() {
        assert!(Config::parse(&["--nope".to_owned()]).is_none());
    }

    #[test]
    fn known_corpora_cover_the_workspace_and_cc0() {
        let ids: Vec<&str> = KNOWN_CORPORA.iter().map(|known| known.id).collect();
        assert!(ids.contains(&"strict"));
        assert!(ids.contains(&"docx"));
        assert!(ids.contains(&"cc0-docx"));
        assert!(ids.contains(&"cc0-rtf"));
        assert!(ids.contains(&"cc0-docx-math"));
        assert!(KNOWN_CORPORA
            .iter()
            .filter(|known| !known.id.starts_with("cc0"))
            .all(|known| known.path.starts_with("strict-ooxml-core/tests/")));
    }

    #[test]
    fn a_known_transitional_corpus_is_recognized_by_its_path() {
        let cc0 = known_for(Path::new("testdata/CC0_DOCX")).expect("cc0 docx");
        assert_eq!(cc0.id, "cc0-docx");
        assert!(cc0.transitional);
        let strict = known_for(Path::new("strict-ooxml-core/tests/strict")).expect("strict");
        assert!(!strict.transitional);
        assert!(known_for(Path::new("somewhere/else")).is_none());
    }

    #[test]
    fn slugs_and_duplicate_ids_stay_url_safe() {
        assert_eq!(slug("CC0_DOCX"), "cc0-docx");
        assert_eq!(slug("..."), "corpus");
        let mut used = BTreeSet::new();
        assert_eq!(unique_id("cc0-docx", &mut used), "cc0-docx");
        assert_eq!(unique_id("cc0-docx", &mut used), "cc0-docx-2");
    }

    #[test]
    fn a_corpus_listing_names_the_id_the_page_selects() {
        let corpus = Corpus {
            id: "cc0-docx".to_owned(),
            label: "CC0 DOCX".to_owned(),
            path: "testdata/CC0_DOCX".to_owned(),
            entries: vec![Entry {
                name: "a.docx".to_owned(),
                path: PathBuf::from("testdata/CC0_DOCX/a.docx"),
                size: 12,
            }],
            transitional: true,
        };
        let json = corpus_json(&corpus);
        assert!(json.contains("\"id\":\"cc0-docx\""));
        assert!(json.contains("\"count\":1"));
        assert!(json.contains("\"transitional\":true"));
    }

    #[test]
    fn a_corpus_listing_does_not_leak_its_directory() {
        let corpus = Corpus {
            id: "private".to_owned(),
            label: "private".to_owned(),
            path: "/home/someone/secret/private".to_owned(),
            entries: Vec::new(),
            transitional: false,
        };
        let json = corpus_json(&corpus);
        assert!(!json.contains("\"path\""), "{json}");
        assert!(!json.contains("/home/someone"), "{json}");
    }

    #[test]
    fn a_job_that_finishes_in_time_returns_its_value() {
        assert_eq!(run_bounded(Duration::from_secs(10), || 7), Ok(7));
    }

    #[test]
    fn a_job_that_does_not_finish_is_abandoned_at_the_timeout() {
        let started = std::time::Instant::now();
        let outcome = run_bounded(Duration::from_millis(50), || {
            std::thread::sleep(Duration::from_secs(5));
        });
        assert_eq!(outcome, Err(Unfinished::TimedOut));
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "the caller must not wait for the job: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_job_that_panics_is_lost_not_fatal() {
        let outcome = run_bounded(Duration::from_secs(10), || -> u8 {
            panic!("renderer bug");
        });
        assert!(matches!(outcome, Err(Unfinished::Lost(_))), "{outcome:?}");
    }

    #[test]
    fn a_document_that_will_not_open_is_a_failed_view() {
        let entry = Entry {
            name: "missing.docx".to_owned(),
            path: PathBuf::from("does/not/exist/missing.docx"),
            size: 0,
        };
        let view = render_bounded(
            &entry,
            false,
            strict_ooxml_view::RenderSettings::new(96.0),
            worker::RtfFlags::default(),
            Duration::from_secs(30),
        );
        assert_eq!(view.name, "missing.docx");
        assert!(view.pages.is_empty());
        assert!(view.note.is_some());
        assert_eq!(view.pipeline.outcome, "failed");
    }

    #[test]
    fn json_strings_are_escaped() {
        assert_eq!(json_string("plain"), "\"plain\"");
        assert_eq!(json_string("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(json_string("a\\b"), "\"a\\\\b\"");
        assert_eq!(json_string("line\nbreak"), "\"line\\nbreak\"");
        // A control character would otherwise produce invalid JSON.
        assert_eq!(json_string("\u{1}"), "\"\\u0001\"");
    }

    #[test]
    fn json_numbers_drop_a_trailing_zero() {
        assert_eq!(json_number(816.0), "816");
        assert_eq!(json_number(1056.5), "1056.5");
        // A non-finite width would collapse the layout, so it is pinned.
        assert_eq!(json_number(f64::NAN), "0");
        assert_eq!(json_number(f64::INFINITY), "0");
    }

    #[test]
    fn a_query_parameter_is_found_and_decoded() {
        assert_eq!(
            query_value("name=%D0%B4%D0%BE%D0%BA.docx", "name").as_deref(),
            Some("док.docx")
        );
        assert_eq!(
            query_value("a=1&name=x.docx", "name").as_deref(),
            Some("x.docx")
        );
        assert_eq!(query_value("a=1", "name"), None);
    }

    #[test]
    fn a_document_with_no_note_and_no_summary_still_produces_valid_json() {
        // The empty-document case is what a menu entry for a file that fails
        // to open looks like, and it is exactly the one that produced
        // `"note":,` — invalid JSON the client could not read.
        let view = DocumentView {
            page_files: None,
            name: "broken.docx".to_owned(),
            conformance: "—".to_owned(),
            summary: None,
            note: None,
            pages: Vec::new(),
            pipeline: empty_pipeline(),
        };
        let json = super::document_json(&view);
        assert!(
            json.contains("\"note\":null"),
            "an absent note must be null: {json}"
        );
        assert!(
            json.contains("\"summary\":null"),
            "an absent summary must be null: {json}"
        );
        assert!(json.contains("\"pages\":[]"), "no pages must be []: {json}");
        // Every value slot is filled: no `"x":,` and no bare `:`.
        assert!(!json.contains(",,"), "an empty value leaked: {json}");
        assert!(!json.contains(":,"), "an empty value leaked: {json}");
    }

    #[test]
    fn a_rendered_document_produces_valid_json() {
        let view = DocumentView {
            page_files: None,
            name: "a.docx".to_owned(),
            conformance: "strict".to_owned(),
            summary: Some(strict_ooxml_view::Summary {
                supported: 3,
                partial: 1,
                unsupported: 0,
                ignored: 2,
                error: 0,
            }),
            note: None,
            pipeline: empty_pipeline(),
            pages: vec![strict_ooxml_view::Rendered {
                number: 1,
                width: 816.0,
                height: 1056.0,
                svg: "<svg a=\"1\"/>".to_owned(),
            }],
        };
        let json = super::document_json(&view);
        assert!(json.contains("\"width\":816"));
        assert!(json.contains("\"svg\":\"<svg a=\\\"1\\\"/>\""));
        assert!(!json.contains(":,"), "{json}");
    }

    #[test]
    fn the_success_exit_code_is_zero() {
        assert_eq!(EXIT_OK, 0);
    }
}
