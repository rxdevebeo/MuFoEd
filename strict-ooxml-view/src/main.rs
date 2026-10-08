//! A local viewer for rendered `WordprocessingML` documents.
//!
//! Scans a directory of `.docx` files, renders whichever one is chosen, and
//! serves the pages as SVG in a scrolling column with the page boundaries
//! visible. Meant for judging a render — whether a glyph landed in the right
//! place — which is why it serves vector pages instead of rasterizing them.
//!
//! ```text
//! strict-ooxml-view [DIR...] [--port N] [--transitional] [--scale N]
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
        clippy::unreachable
    )
)]

mod http;
mod ui;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, PoisonError};

use http::Response;
use strict_ooxml_view::{discover, failed, render, Cache, DocumentView, Entry};

/// Exit code: the viewer ran and stopped cleanly.
const EXIT_OK: u8 = 0;
/// Exit code: the viewer could not start.
const EXIT_ERROR: u8 = 2;

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
        id: "cc0",
        label: "CC0",
        path: "testdata/CC0",
        transitional: true,
    },
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
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
    /// Directory, as printed at startup and returned by the API.
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
    /// Output scale, in px per inch.
    ///
    /// This is a DPI, not a multiplier: the renderer converts twips by
    /// dividing by 1440 and multiplying by this. Passing 1.0 renders an
    /// 8.5x11 inch page as 8.5x11 *pixels* — a postage stamp — and the
    /// content then overflows into a dozen pages.
    scale: f64,
}

impl State {
    /// Returns the view for `name` in `corpus`, rendering it on first request.
    fn view(&self, corpus: &Corpus, name: &str) -> Option<DocumentView> {
        if let Some(cached) = self
            .caches
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&corpus.id)
            .and_then(|cache| cache.get(name).cloned())
        {
            return Some(cached);
        }
        let entry = corpus.entries.iter().find(|entry| entry.name == name)?;
        // A document that will not open is still a menu entry that has to say
        // so; showing nothing would look like a viewer bug.
        let view = match render(entry, corpus.transitional, self.scale) {
            Ok(view) => view,
            Err(error) => failed(entry, &error.to_string()),
        };
        self.caches
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(corpus.id.clone())
            .or_default()
            .insert(view.clone());
        Some(view)
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
            Response::ok(
                "application/json; charset=utf-8",
                document_json(&view).into_bytes(),
            )
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
        "{{\"id\":{},\"label\":{},\"path\":{},\"count\":{},\"transitional\":{}}}",
        json_string(&corpus.id),
        json_string(&corpus.label),
        json_string(&corpus.path),
        corpus.entries.len(),
        corpus.transitional
    )
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
    /// Directories given on the command line, in order.
    dirs: Vec<PathBuf>,
    /// Port to bind.
    port: u16,
    /// Whether to normalize every corpus, including Strict.
    transitional: bool,
    /// Render scale.
    scale: f64,
}

impl Config {
    /// Parses `args`, or returns `None` after printing what was wrong.
    fn parse(args: &[String]) -> Option<Self> {
        let mut config = Self {
            dirs: Vec::new(),
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
    println!("usage: strict-ooxml-view [DIR...] [--port N] [--scale N] [--transitional]");
    println!();
    println!("  DIR...            directories of .docx files");
    println!("                    (default: every known corpus that contains documents)");
    println!("  --port N          port to bind on 127.0.0.1 (default 8181)");
    println!("  --scale N         output DPI (default 96; 144 doubles the page size)");
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
        corpus_json, json_number, json_string, known_for, query_value, slug, unique_id, Config,
        Corpus, Entry, EXIT_OK, KNOWN_CORPORA,
    };
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
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
        assert!(KNOWN_CORPORA
            .iter()
            .filter(|known| known.id != "cc0" && known.id != "cc0-docx")
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
