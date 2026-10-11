//! Document discovery and the page cache behind the viewer.
//!
//! A document is rendered on first request and its SVG pages are then served
//! from a temporary disk store by the HTTP worker, or memory by library callers.
//! Temporary pages are owned by their cached view and removed on eviction.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use strict_ooxml::{
    ConformancePolicy, OpenControl, OpenOptions, PageSelection, RenderOptions, StrictDocument,
    TransitionalNormalizer,
};
use strict_ooxml_core::error::Result;
use strict_ooxml_core::pipeline::{PipelineIssue, PipelineStage, PipelineSummary};

/// One `.docx` the viewer can open.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// File name, used as the menu label and the URL key.
    pub name: String,
    /// Absolute path, resolved once so a later working-directory change cannot
    /// make a cached entry point somewhere else.
    pub path: PathBuf,
    /// Size in bytes, for the menu.
    pub size: u64,
}

/// Lists the `.docx` files directly inside `dir`, sorted by name.
///
/// Not recursive: the viewer is pointed at a corpus, and a recursive walk
/// would pick up temporary copies and nested build output. Word lock files
/// (`~$…`) are not documents and are left out of the menu.
#[must_use]
pub fn discover(dir: &Path) -> Vec<Entry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = read
        .flatten()
        .filter_map(|item| {
            let path = item.path();
            let is_docx = path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("docx"));
            if !is_docx || !path.is_file() {
                return None;
            }
            let name = path.file_name().map_or_else(
                || "<unnamed>".to_owned(),
                |name| name.to_string_lossy().into_owned(),
            );
            // Office creates `~$name.docx` beside an open document. It is a
            // lock, not a second copy of the text.
            if name.starts_with("~$") {
                return None;
            }
            let size = item.metadata().map_or(0, |meta| meta.len());
            Some(Entry { name, path, size })
        })
        .collect();
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    entries
}

/// Rendered pages of one document.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rendered {
    /// Page number in the document, starting at 1.
    pub number: usize,
    /// Page width in px at the requested scale.
    pub width: f64,
    /// Page height in px.
    pub height: f64,
    /// The page's SVG.
    pub svg: String,
}

/// The counts a reader wants next to a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// Fully supported constructs.
    pub supported: u32,
    /// Supported with a documented limitation.
    pub partial: u32,
    /// Recognised but not implemented.
    pub unsupported: u32,
    /// Deliberately out of scope.
    pub ignored: u32,
    /// Could not be modelled.
    pub error: u32,
}

impl Summary {
    /// Whether the document has anything that blocks a faithful render.
    #[cfg_attr(not(test), allow(dead_code))]
    #[must_use]
    pub fn has_blockers(self) -> bool {
        self.unsupported > 0 || self.error > 0
    }
}

/// One stage of the viewer pipeline and whether this process ran it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageStatus {
    /// Stage name: `input`, `normalize`, `convert`, `write`, or `render`.
    pub stage: String,
    /// `ran`, `not_run`, or `failed`.
    pub status: String,
}

/// One issue the viewer shows beside a successful or failed open.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewIssue {
    /// Stage that recorded the issue.
    pub stage: String,
    /// Stable id from that stage.
    pub id: String,
    /// The stage's own severity word.
    pub severity: String,
    /// Package part, when the stage named one.
    pub part: Option<String>,
    /// Page number, when the stage named one.
    pub page: Option<u32>,
    /// Location text, when the stage named one.
    pub location: Option<String>,
    /// How many times this issue was counted.
    pub count: u32,
    /// The stage's own explanation.
    pub detail: String,
}

/// The live pipeline report for one opened document.
///
/// The support [`Summary`] stays a separate count of mechanisms. This report
/// is the loss ledger. A sidecar is attached only when its output hash is the
/// file being viewed; it never clears a live issue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineView {
    /// `clean`, `degraded`, or `failed`.
    pub outcome: String,
    /// Issues from the stages that ran, plus a matching sidecar.
    pub issues: Vec<ViewIssue>,
    /// What this viewer process itself ran. Convert and write stay `not_run`.
    pub stages: Vec<StageStatus>,
    /// `absent`, `matched`, or `rejected`.
    pub sidecar: String,
}

/// A rendered document, ready to serve.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentView {
    /// File name shown in the header.
    pub name: String,
    /// Conformance as the package reports it.
    pub conformance: String,
    /// Support summary, when the document could be opened.
    pub summary: Option<Summary>,
    /// Failure reason, when rendering could not finish.
    pub note: Option<String>,
    /// Pages, empty when the document could not be opened.
    pub pages: Vec<Rendered>,
    /// Normalization and render losses. Present for a failed open too.
    pub pipeline: PipelineView,
    /// Temporary SVG storage, shared until the last cached view is released.
    #[serde(skip)]
    pub page_files: Option<Arc<tempfile::TempDir>>,
}

impl DocumentView {
    /// Reads one page from its temporary store or the in-memory library result.
    ///
    /// # Errors
    /// Returns a filesystem error if a stored page is no longer readable.
    pub fn page_svg(&self, page: &Rendered) -> std::io::Result<String> {
        self.page_files.as_ref().map_or_else(
            || Ok(page.svg.clone()),
            |directory| {
                std::fs::read_to_string(directory.path().join(format!("{}.svg", page.number)))
            },
        )
    }

    /// Total stored SVG bytes, without loading the page contents.
    #[must_use]
    pub fn svg_bytes(&self) -> u64 {
        self.pages
            .iter()
            .map(|page| {
                self.page_files.as_ref().map_or_else(
                    || u64::try_from(page.svg.len()).unwrap_or(u64::MAX),
                    |directory| {
                        std::fs::metadata(directory.path().join(format!("{}.svg", page.number)))
                            .map_or(0, |metadata| metadata.len())
                    },
                )
            })
            .sum()
    }
}

/// Renders `entry`, normalizing it when `transitional` is set.
///
/// # Errors
///
/// Returns the underlying error when the document cannot be opened or
/// rendered. The caller turns that into a page with a note rather than a
/// failed request, so one bad document does not break the viewer.
pub fn render(entry: &Entry, transitional: bool, scale: f64) -> Result<DocumentView> {
    render_controlled(entry, transitional, scale, &OpenControl::new())
}

/// Renders with a shared cancellation/deadline handle for both opening and painting.
pub fn render_controlled(
    entry: &Entry,
    transitional: bool,
    scale: f64,
    control: &OpenControl,
) -> Result<DocumentView> {
    let options = if transitional {
        OpenOptions::default()
            .conformance(ConformancePolicy::Normalize)
            .shared_normalization(Arc::new(TransitionalNormalizer::new()))
    } else {
        OpenOptions::default()
    }
    .control(control.clone());
    let document = StrictDocument::open_path(&entry.path, &options)?;
    let mut pipeline = pipeline_for(&document, &entry.path);
    let conformance = format!("{:?}", document.package().conformance()).to_lowercase();
    let report = document.support_report();
    let summary = Summary {
        supported: report.summary.supported,
        partial: report.summary.partial,
        unsupported: report.summary.unsupported,
        ignored: report.summary.ignored,
        error: report.summary.error,
    };
    let render_options = RenderOptions::default()
        .control(control.clone())
        .scale(scale)
        .floating(true)
        .math(true)
        .pages(PageSelection::All);
    let pages = document.render_svg(&render_options)?;
    let mut layout_warnings = BTreeSet::new();
    for page in &pages {
        for warning in &page.warnings {
            // Layout diagnostics are attached to every SVG page by the render
            // API. Record them once without inventing a per-page location.
            let page_specific = warning.starts_with("render.invalid-xml-char:");
            if !page_specific && !layout_warnings.insert(warning.as_str()) {
                continue;
            }
            pipeline.issues.push(ViewIssue {
                stage: "render".to_owned(),
                id: warning
                    .split(':')
                    .next()
                    .unwrap_or("render.warning")
                    .to_owned(),
                severity: "warning".to_owned(),
                part: None,
                page: if page_specific {
                    u32::try_from(page.index + 1).ok()
                } else {
                    None
                },
                location: None,
                count: 1,
                detail: warning.clone(),
            });
            if pipeline.outcome != "failed" {
                "degraded".clone_into(&mut pipeline.outcome);
            }
        }
    }
    let rendered = pages
        .into_iter()
        .map(|page| Rendered {
            number: page.index + 1,
            width: page.width_px,
            height: page.height_px,
            svg: page.svg,
        })
        .collect();
    Ok(DocumentView {
        name: entry.name.clone(),
        conformance,
        summary: Some(summary),
        note: None,
        pages: rendered,
        pipeline,
        page_files: None,
    })
}

/// A view describing a document that could not be opened.
///
/// Built here rather than at the call site so the reason is always recorded.
#[must_use]
pub fn failed(entry: &Entry, reason: &str) -> DocumentView {
    DocumentView {
        page_files: None,
        name: entry.name.clone(),
        conformance: "—".to_owned(),
        summary: None,
        note: Some(reason.to_owned()),
        pages: Vec::new(),
        pipeline: PipelineView {
            outcome: "failed".to_owned(),
            issues: vec![ViewIssue {
                stage: "input".to_owned(),
                id: "open".to_owned(),
                severity: "error".to_owned(),
                part: None,
                page: None,
                location: None,
                count: 1,
                detail: reason.to_owned(),
            }],
            stages: stage_rows("failed", false, false),
            sidecar: "absent".to_owned(),
        },
    }
}

/// Builds the live report, then attaches a sidecar only when its hash matches.
fn pipeline_for(document: &StrictDocument, path: &Path) -> PipelineView {
    let normalization = document.package().normalization_report();
    let mut summary = normalization
        .as_ref()
        .map_or_else(PipelineSummary::new, |report| {
            PipelineSummary::from_normalization(PipelineStage::Normalize, report)
        });
    let sidecar = attach_sidecar(path, &mut summary);
    PipelineView {
        outcome: summary.outcome.to_string(),
        issues: summary.issues.iter().map(view_issue).collect(),
        stages: stage_rows("ran", normalization.is_some(), true),
        sidecar: sidecar.to_owned(),
    }
}

fn stage_rows(input: &str, normalize_ran: bool, render_ran: bool) -> Vec<StageStatus> {
    let status = |stage: &str, value: &str| StageStatus {
        stage: stage.to_owned(),
        status: value.to_owned(),
    };
    vec![
        status("input", input),
        status("normalize", if normalize_ran { "ran" } else { "not_run" }),
        status("convert", "not_run"),
        status("write", "not_run"),
        status("render", if render_ran { "ran" } else { "not_run" }),
    ]
}

fn view_issue(issue: &PipelineIssue) -> ViewIssue {
    ViewIssue {
        stage: issue.stage.to_string(),
        id: issue.id.clone(),
        severity: issue.severity.clone(),
        part: issue.part.clone(),
        page: issue.page,
        location: issue.location.clone(),
        count: issue.count,
        detail: issue.detail.clone(),
    }
}

/// Reads `{file}.report.json`. A hash that is not this file's is ignored.
fn attach_sidecar(path: &Path, summary: &mut PipelineSummary) -> &'static str {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return "absent";
    };
    let sidecar = path.with_file_name(format!("{name}.report.json"));
    let Ok(text) = std::fs::read_to_string(&sidecar) else {
        return "absent";
    };
    let Some(hash) = quoted_field(&text, "output_sha256") else {
        return "rejected";
    };
    let Ok(bytes) = std::fs::read(path) else {
        return "rejected";
    };
    if hash != sha256_hex(&bytes) {
        return "rejected";
    }
    let existing: Vec<(String, String, String)> = summary
        .issues
        .iter()
        .map(|issue| {
            (
                issue.stage.to_string(),
                issue.id.clone(),
                issue.detail.clone(),
            )
        })
        .collect();
    let extra: Vec<PipelineIssue> = sidecar_issues(&text)
        .into_iter()
        .filter(|issue| {
            !existing.iter().any(|(stage, id, detail)| {
                stage == &issue.stage.to_string() && id == &issue.id && detail == &issue.detail
            })
        })
        .collect();
    summary.extend_issues(extra);
    "matched"
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        text.push(hex_digit(byte >> 4));
        text.push(hex_digit(byte & 0x0f));
    }
    text
}

/// A lowercase hex digit for a nibble; the fallback is never taken.
fn hex_digit(nibble: u8) -> char {
    char::from_digit(u32::from(nibble), 16).unwrap_or('0')
}

fn quoted_field(text: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\"");
    let start = text.find(&pattern)?;
    let after = text.get(start + pattern.len()..)?;
    let colon = after.find(':')?;
    let rest = after.get(colon + 1..)?.trim_start();
    let body = rest.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            out.push(chars.next()?);
        } else if character == '"' {
            return Some(out);
        } else {
            out.push(character);
        }
    }
    None
}

fn sidecar_issues(text: &str) -> Vec<PipelineIssue> {
    let mut issues = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("\"stage\"") {
        // `find` returns char boundaries, so neither split can fail.
        let Some(slice) = rest.get(start..) else {
            break;
        };
        let end = slice.find('}').map_or(slice.len(), |index| index);
        let Some((object, tail)) = slice.split_at_checked(end) else {
            break;
        };
        rest = tail;
        let Some(stage) = quoted_field(object, "stage")
            .as_deref()
            .and_then(stage_named)
        else {
            continue;
        };
        let Some(id) = quoted_field(object, "id") else {
            continue;
        };
        issues.push(PipelineIssue {
            stage,
            id,
            severity: quoted_field(object, "severity").unwrap_or_else(|| "info".to_owned()),
            part: quoted_field(object, "part"),
            page: number_field(object, "page"),
            location: quoted_field(object, "location"),
            count: number_field(object, "count").unwrap_or(1),
            detail: quoted_field(object, "detail").unwrap_or_default(),
        });
    }
    issues
}

fn number_field(text: &str, key: &str) -> Option<u32> {
    let pattern = format!("\"{key}\"");
    let start = text.find(&pattern)?;
    let after = text.get(start + pattern.len()..)?;
    let colon = after.find(':')?;
    let rest = after.get(colon + 1..)?.trim_start();
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn stage_named(name: &str) -> Option<PipelineStage> {
    match name {
        "input" => Some(PipelineStage::Input),
        "normalize" => Some(PipelineStage::Normalize),
        "convert" => Some(PipelineStage::Convert),
        "write" => Some(PipelineStage::Write),
        "render" => Some(PipelineStage::Render),
        _ => None,
    }
}

/// The in-memory page cache.
///
/// Keyed by file name. Rendering is expensive enough that re-rendering on
/// every page navigation would make the viewer unusable. Each corpus retains
/// at most eight documents and 64 MiB of SVG and diagnostic string payloads.
#[derive(Debug)]
pub struct Cache {
    views: BTreeMap<String, Arc<DocumentView>>,
    order: VecDeque<String>,
    bytes: usize,
    max_bytes: usize,
    max_documents: usize,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            views: BTreeMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            max_bytes: 64 * 1024 * 1024,
            max_documents: 8,
        }
    }
}

fn view_bytes(view: &DocumentView) -> usize {
    view.pages
        .iter()
        .map(|page| page.svg.len() + std::mem::size_of::<Rendered>())
        .sum::<usize>()
        + view
            .pipeline
            .issues
            .iter()
            .map(|issue| issue.detail.len() + issue.id.len())
            .sum::<usize>()
        + view.note.as_ref().map_or(0, String::len)
        + view.name.len()
}

impl Cache {
    /// Creates an empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a cached view.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&DocumentView> {
        self.views.get(name).map(Arc::as_ref)
    }

    /// Shares a cached result without copying its SVG payload.
    #[must_use]
    pub fn get_shared(&self, name: &str) -> Option<Arc<DocumentView>> {
        self.views.get(name).cloned()
    }

    /// Stores a view, evicting oldest entries past eight documents or 64 MiB.
    /// A larger single view is served without caching it.
    pub fn insert(&mut self, view: DocumentView) {
        self.insert_shared(Arc::new(view));
    }

    /// Retains the same allocation used by the response handler.
    pub fn insert_shared(&mut self, view: Arc<DocumentView>) {
        if let Some(previous) = self.views.remove(&view.name) {
            self.bytes = self.bytes.saturating_sub(view_bytes(&previous));
            self.order.retain(|name| name != &view.name);
        }
        let size = view_bytes(&view);
        if size > self.max_bytes || self.max_documents == 0 {
            return;
        }
        while self.bytes.saturating_add(size) > self.max_bytes
            || self.views.len() >= self.max_documents
        {
            let Some(name) = self.order.pop_front() else {
                break;
            };
            if let Some(previous) = self.views.remove(&name) {
                self.bytes = self.bytes.saturating_sub(view_bytes(&previous));
            }
        }
        self.bytes += size;
        self.order.push_back(view.name.clone());
        self.views.insert(view.name.clone(), view);
    }

    /// How many documents are rendered.
    #[cfg_attr(not(test), allow(dead_code))]
    #[must_use]
    pub fn len(&self) -> usize {
        self.views.len()
    }

    /// Whether nothing has been rendered yet.
    #[cfg_attr(not(test), allow(dead_code))]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{discover, Cache, DocumentView, PipelineView, Summary};
    use std::path::Path;

    #[test]
    fn an_empty_directory_yields_nothing() {
        let dir = std::env::temp_dir().join("strict-view-missing-directory");
        assert!(discover(&dir).is_empty());
    }

    #[test]
    fn discovery_lists_only_docx_files_sorted() {
        let dir = std::env::temp_dir().join("strict-view-discover-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create");
        std::fs::write(dir.join("b.docx"), b"x").expect("write");
        std::fs::write(dir.join("a.docx"), b"x").expect("write");
        std::fs::write(dir.join("notes.txt"), b"x").expect("write");
        std::fs::write(dir.join("~$a.docx"), b"x").expect("write");
        std::fs::create_dir_all(dir.join("nested")).expect("nested");
        std::fs::write(dir.join("nested/c.docx"), b"x").expect("write");
        let found = discover(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        let names: Vec<&str> = found.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["a.docx", "b.docx"],
            "sorted, docx only, not recursive, no Word lock files"
        );
    }

    #[test]
    fn discovery_survives_a_non_ascii_name() {
        let dir = std::env::temp_dir().join("strict-view-utf8-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create");
        // A Cyrillic name, which half a real corpus has.
        let name = "документ.docx";
        std::fs::write(dir.join(name), b"x").expect("write");
        let found = discover(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, name);
    }

    #[test]
    fn the_cache_stores_and_returns_by_name() {
        let mut cache = Cache::new();
        assert!(cache.is_empty());
        let view = DocumentView {
            page_files: None,
            name: "a.docx".to_owned(),
            conformance: "strict".to_owned(),
            summary: None,
            note: None,
            pages: Vec::new(),
            pipeline: PipelineView {
                outcome: "clean".to_owned(),
                issues: Vec::new(),
                stages: Vec::new(),
                sidecar: "absent".to_owned(),
            },
        };
        cache.insert(view);
        assert_eq!(cache.len(), 1);
        assert!(cache.get("a.docx").is_some());
        assert!(std::sync::Arc::ptr_eq(
            &cache.get_shared("a.docx").expect("cached"),
            &cache.get_shared("a.docx").expect("cached")
        ));
        assert!(cache.get("missing.docx").is_none());
    }

    #[test]
    fn disk_pages_survive_shared_views_and_are_removed_on_eviction() {
        let directory = tempfile::tempdir().expect("temporary pages");
        let path = directory.path().to_path_buf();
        std::fs::write(path.join("201.svg"), "<svg>last page</svg>").expect("write page");
        let entry = super::Entry {
            name: "long.docx".into(),
            path: "unused".into(),
            size: 0,
        };
        let mut view = super::failed(&entry, "fixture");
        view.pages.push(super::Rendered {
            number: 201,
            width: 816.0,
            height: 1056.0,
            svg: String::new(),
        });
        view.page_files = Some(std::sync::Arc::new(directory));
        let shared = std::sync::Arc::new(view);
        let mut cache = Cache::new();
        cache.insert_shared(std::sync::Arc::clone(&shared));
        assert_eq!(
            shared
                .page_svg(shared.pages.first().expect("page"))
                .expect("read"),
            "<svg>last page</svg>"
        );
        assert_eq!(shared.svg_bytes(), 20);
        drop(cache);
        assert!(path.exists(), "active response keeps its stored pages");
        drop(shared);
        assert!(!path.exists(), "last owner removes stored pages");
    }

    #[test]
    fn a_view_for_a_document_that_failed_open_records_the_reason() {
        let entry = super::Entry {
            name: "broken.docx".to_owned(),
            path: Path::new("/nowhere/broken.docx").to_path_buf(),
            size: 0,
        };
        let view = super::failed(&entry, "damaged zip");
        assert_eq!(view.note.as_deref(), Some("damaged zip"));
        assert!(view.pages.is_empty());
        assert_eq!(view.pipeline.outcome, "failed");
        assert_eq!(view.pipeline.issues[0].detail, "damaged zip");
    }

    #[test]
    fn blockers_are_unsupported_or_error() {
        let clean = Summary {
            supported: 10,
            partial: 2,
            unsupported: 0,
            ignored: 3,
            error: 0,
        };
        let with_error = Summary { error: 1, ..clean };
        assert!(!clean.has_blockers());
        assert!(with_error.has_blockers());
    }

    #[test]
    fn cache_evicts_by_bytes_and_does_not_retain_oversized_views() {
        let mut cache = Cache {
            max_bytes: 20,
            max_documents: 2,
            ..Cache::new()
        };
        let entry = super::Entry {
            name: "a".to_owned(),
            path: "missing".into(),
            size: 0,
        };
        let mut view = super::failed(&entry, "");
        view.pipeline.issues.clear();
        view.note = Some("123456789".to_owned());
        cache.insert(view.clone());
        view.name = "b".to_owned();
        cache.insert(view.clone());
        view.name = "c".to_owned();
        cache.insert(view.clone());
        assert!(cache.get("a").is_none());
        assert!(cache.get("b").is_some());
        assert!(cache.get("c").is_some());
        view.name = "huge".to_owned();
        view.note = Some("x".repeat(21));
        cache.insert(view);
        assert!(cache.get("huge").is_none());
        assert_eq!(cache.bytes, 20);
    }

    #[test]
    fn cancelled_control_is_used_by_the_viewer_open() {
        let dir = std::env::temp_dir().join(format!("view-control-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("plain.docx");
        std::fs::write(
            &path,
            strict_ooxml_testkit::DocxBuilder::strict()
                .body("<w:p><w:r><w:t>plain</w:t></w:r></w:p>")
                .build(),
        )
        .expect("write");
        let control = strict_ooxml::OpenControl::new();
        control.cancel();
        let entry = super::Entry {
            name: "plain.docx".to_owned(),
            path,
            size: 0,
        };
        let result = super::render_controlled(&entry, false, 96.0, &control);
        assert!(matches!(
            result,
            Err(strict_ooxml::StrictError::Cancelled { .. })
        ));
        std::fs::remove_dir_all(dir).expect("clean");
    }

    #[test]
    fn render_warning_degrades_the_pipeline_without_hiding_pages() {
        let dir = std::env::temp_dir().join(format!("view-warning-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("pattern.docx");
        std::fs::write(
            &path,
            strict_ooxml_testkit::graphics::fill_docx(
                "rect",
                &strict_ooxml_testkit::graphics::pattern("unknownPattern"),
            ),
        )
        .expect("write");
        let entry = super::Entry {
            name: "pattern.docx".to_owned(),
            path,
            size: 0,
        };
        let view = super::render(&entry, false, 96.0).expect("render");
        assert_eq!(view.pages.len(), 1);
        assert_eq!(view.pipeline.outcome, "degraded");
        assert!(view
            .pipeline
            .issues
            .iter()
            .any(|issue| issue.stage == "render"
                && issue.id == "render.pattern"
                && issue.detail.contains("unknownPattern")
                && issue.page.is_none()));
        std::fs::remove_dir_all(dir).expect("clean");
    }

    #[test]
    fn all_pages_are_retained_without_preview_truncation() {
        let dir = std::env::temp_dir().join(format!("view-cap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("pages.docx");
        // Cross the former 200-page cutoff with a small, deterministic document.
        for page_count in [2, 201] {
            let mut body = String::from("<w:p><w:r><w:t>first</w:t></w:r></w:p>");
            body.push_str(
                &"<w:p><w:r><w:br w:type=\"page\"/><w:t>page</w:t></w:r></w:p>"
                    .repeat(page_count - 1),
            );
            std::fs::write(
                &path,
                strict_ooxml_testkit::DocxBuilder::strict()
                    .body(&body)
                    .build(),
            )
            .expect("write");
            let entry = super::Entry {
                name: "pages.docx".to_owned(),
                path: path.clone(),
                size: 0,
            };
            let view =
                super::render_controlled(&entry, false, 96.0, &strict_ooxml::OpenControl::new())
                    .expect("render");
            assert_eq!(view.pages.len(), page_count);
            assert!(view.note.is_none());
            assert_eq!(
                view.pipeline
                    .issues
                    .iter()
                    .filter(|issue| issue.id == "render.pgSz")
                    .count(),
                1,
                "a document-wide layout warning must not be multiplied by page count"
            );
        }
        std::fs::remove_dir_all(dir).expect("clean");
    }
}
