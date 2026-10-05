//! A local viewer for rendered `WordprocessingML` documents.
//!
//! Scans a directory of `.docx` files, renders whichever one is chosen, and
//! serves the pages as SVG in a scrolling column with the page boundaries
//! visible. Meant for judging a render — whether a glyph landed in the right
//! place — which is why it serves vector pages instead of rasterizing them.
//!
//! ```text
//! strict-ooxml-view [DIR] [--port N] [--transitional] [--scale N]
//! ```
//!
//! With no directory it looks in the workspace's two corpora, so it can be run
//! from a checkout without arguments.

mod http;
mod ui;

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use http::Response;
use strict_ooxml_view::{discover, failed, render, Cache, DocumentView, Entry};

/// Exit code: the viewer ran and stopped cleanly.
const EXIT_OK: u8 = 0;
/// Exit code: the viewer could not start.
const EXIT_ERROR: u8 = 2;

/// Directories searched when none is given.
const DEFAULT_DIRS: &[&str] = &[
    "strict-ooxml-core/tests/strict",
    "strict-ooxml-core/tests/docx",
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(config) = Config::parse(&args) else {
        return ExitCode::from(EXIT_ERROR);
    };

    let Some(dir) = config.directory() else {
        eprintln!("error: no document directory found");
        eprintln!("hint: pass one explicitly, e.g. strict-ooxml-view path/to/corpus");
        return ExitCode::from(EXIT_ERROR);
    };
    let entries = discover(&dir);
    if entries.is_empty() {
        eprintln!("error: no .docx files in {}", dir.display());
        return ExitCode::from(EXIT_ERROR);
    }

    let state = Arc::new(State {
        entries: entries.clone(),
        cache: Mutex::new(Cache::new()),
        transitional: config.transitional,
        scale: config.scale,
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
    println!(
        "serving {} document(s) from {}",
        entries.len(),
        dir.display()
    );
    println!("open http://127.0.0.1:{port}/");
    if config.transitional {
        println!("(--transitional: Transitional packages are normalized to Strict)");
    }
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

/// Everything the routes need.
struct State {
    /// Discovered documents, in menu order.
    entries: Vec<Entry>,
    /// Rendered pages so far.
    cache: Mutex<Cache>,
    /// Whether to normalize Transitional input.
    transitional: bool,
    /// Output scale, in px per inch.
    ///
    /// This is a DPI, not a multiplier: the renderer converts twips by
    /// dividing by 1440 and multiplying by this. Passing 1.0 renders an
    /// 8.5x11 inch page as 8.5x11 *pixels* — a postage stamp — and the
    /// content then overflows into a dozen pages.
    scale: f64,
}

impl State {
    /// Returns the view for `name`, rendering it on first request.
    fn view(&self, name: &str) -> Option<DocumentView> {
        if let Some(cached) = self.cache.lock().expect("viewer cache").get(name) {
            return Some(cached.clone());
        }
        let entry = self.entries.iter().find(|entry| entry.name == name)?;
        // A document that will not open is still a menu entry that has to say
        // so; showing nothing would look like a viewer bug.
        let view = match render(entry, self.transitional, self.scale) {
            Ok(view) => view,
            Err(error) => failed(entry, &error.to_string()),
        };
        self.cache
            .lock()
            .expect("viewer cache")
            .insert(view.clone());
        Some(view)
    }
}

/// Answers one request.
fn route(state: &State, path: &str, query: &str) -> Response {
    match path {
        "/" | "/index.html" => Response::ok("text/html; charset=utf-8", ui::page().into_bytes()),
        "/api/documents" => {
            let listing: Vec<String> = state
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
            let name = query_value(query, "name").unwrap_or_default();
            if name.is_empty() {
                return Response::bad_request("`name` is required");
            }
            let Some(view) = state.view(&name) else {
                return Response::not_found(&name);
            };
            Response::ok(
                "application/json; charset=utf-8",
                document_json(&view).into_bytes(),
            )
        }
        other => Response::not_found(other),
    }
}

/// One menu entry as JSON.
fn menu_entry_json(name: &str, size: u64) -> String {
    format!("{{\"name\":{},\"size\":{size}}}", json_string(name))
}

/// A rendered view as the JSON the page script expects.
fn document_json(view: &DocumentView) -> String {
    let pages: Vec<String> = view
        .pages
        .iter()
        .map(|page| {
            format!(
                "{{\"number\":{},\"width\":{},\"height\":{},\"svg\":{}}}",
                page.number,
                json_number(page.width),
                json_number(page.height),
                json_string(&page.svg)
            )
        })
        .collect();
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
    format!(
        "{{\"name\":{},\"conformance\":{},\"summary\":{summary},\"note\":{note},\"pipeline\":{},\"pages\":[{}]}}",
        json_string(&view.name),
        json_string(&view.conformance),
        pipeline_json(&view.pipeline),
        pages.join(",")
    )
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
    out
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
    /// Directory given on the command line.
    dir: Option<PathBuf>,
    /// Port to bind.
    port: u16,
    /// Whether to normalize Transitional input.
    transitional: bool,
    /// Render scale.
    scale: f64,
}

impl Config {
    /// Parses `args`, or returns `None` after printing what was wrong.
    fn parse(args: &[String]) -> Option<Self> {
        let mut config = Self {
            dir: None,
            port: 8181,
            transitional: false,
            scale: 96.0,
        };
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
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
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                other if other.starts_with('-') => {
                    eprintln!("error: unknown option '{other}'");
                    print_usage();
                    return None;
                }
                other => config.dir = Some(PathBuf::from(other)),
            }
            index += 1;
        }
        Some(config)
    }

    /// The directory to serve, falling back to the workspace corpora.
    fn directory(&self) -> Option<PathBuf> {
        if let Some(dir) = &self.dir {
            return Some(dir.clone());
        }
        DEFAULT_DIRS
            .iter()
            .map(PathBuf::from)
            .find(|dir| !discover(dir).is_empty())
    }
}

fn print_usage() {
    println!("usage: strict-ooxml-view [DIR] [--port N] [--scale N] [--transitional]");
    println!();
    println!("  DIR               directory of .docx files (default: the workspace corpora)");
    println!("  --port N          port to bind on 127.0.0.1 (default 8181)");
    println!("  --scale N         output DPI (default 96; 144 doubles the page size)");
    println!("  --transitional    normalize Transitional packages to Strict on the way in");
}

#[cfg(test)]
mod tests {
    use super::{json_number, json_string, query_value, Config, DEFAULT_DIRS, EXIT_OK};
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
        assert_eq!(config.dir, None);
    }

    #[test]
    fn options_are_parsed() {
        let args: Vec<String> = [
            "corpus",
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
        assert_eq!(config.dir.as_deref(), Some(std::path::Path::new("corpus")));
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
    fn the_fallback_corpora_are_the_workspace_test_directories() {
        assert!(DEFAULT_DIRS
            .iter()
            .all(|dir| dir.starts_with("strict-ooxml-core/tests/")));
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
