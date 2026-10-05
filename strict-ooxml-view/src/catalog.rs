//! Document discovery and the page cache behind the viewer.
//!
//! A document is rendered on first request and its SVG pages are then served
//! from memory. Nothing is written to disk: the viewer is a local inspection
//! tool, and a cache directory would outlive the process with stale copies of
//! documents that have since changed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use strict_ooxml::{
    ConformancePolicy, OpenOptions, RenderOptions, StrictDocument, TransitionalNormalizer,
};
use strict_ooxml_core::error::Result;
use strict_ooxml_core::pipeline::{PipelineIssue, PipelineStage, PipelineSummary};

/// One `.docx` the viewer can open.
#[derive(Clone, Debug, PartialEq, Eq)]
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
/// would pick up temporary copies and nested build output.
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
            let size = item.metadata().map_or(0, |meta| meta.len());
            Some(Entry {
                name: path.file_name().map_or_else(
                    || "<unnamed>".to_owned(),
                    |name| name.to_string_lossy().into_owned(),
                ),
                path,
                size,
            })
        })
        .collect();
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    entries
}

/// Rendered pages of one document.
#[derive(Clone, Debug)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageStatus {
    /// Stage name: `input`, `normalize`, `convert`, `write`, or `render`.
    pub stage: String,
    /// `ran`, `not_run`, or `failed`.
    pub status: String,
}

/// One issue the viewer shows beside a successful or failed open.
#[derive(Clone, Debug, PartialEq, Eq)]
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
#[derive(Clone, Debug, PartialEq, Eq)]
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
#[derive(Clone, Debug)]
pub struct DocumentView {
    /// File name shown in the header.
    pub name: String,
    /// Conformance as the package reports it.
    pub conformance: String,
    /// Support summary, when the document could be opened.
    pub summary: Option<Summary>,
    /// Why the document could not be opened, when it could not.
    pub note: Option<String>,
    /// Pages, empty when the document could not be opened.
    pub pages: Vec<Rendered>,
    /// Normalization and render losses. Present for a failed open too.
    pub pipeline: PipelineView,
}

/// Renders `entry`, normalizing it when `transitional` is set.
///
/// # Errors
///
/// Returns the underlying error when the document cannot be opened or
/// rendered. The caller turns that into a page with a note rather than a
/// failed request, so one bad document does not break the viewer.
pub fn render(entry: &Entry, transitional: bool, scale: f64) -> Result<DocumentView> {
    let options = if transitional {
        OpenOptions::default()
            .conformance(ConformancePolicy::Normalize)
            .shared_normalization(Arc::new(TransitionalNormalizer::new()))
    } else {
        OpenOptions::default()
    };
    let document = StrictDocument::open_path(&entry.path, &options)?;
    let pipeline = pipeline_for(&document, &entry.path);
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
        .scale(scale)
        .floating(true)
        .math(true);
    let pages = document.render_svg(&render_options)?;
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
    })
}

/// A view describing a document that could not be opened.
///
/// Built here rather than at the call site so the reason is always recorded.
#[must_use]
pub fn failed(entry: &Entry, reason: &str) -> DocumentView {
    DocumentView {
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
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    text
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
        let slice = &rest[start..];
        let end = slice.find('}').map_or(slice.len(), |index| index);
        let object = &slice[..end];
        rest = &slice[end..];
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
/// every page navigation would make the viewer unusable, and cheap enough
/// that a cache which never evicts is fine for a local tool.
#[derive(Debug, Default)]
pub struct Cache {
    views: BTreeMap<String, DocumentView>,
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
        self.views.get(name)
    }

    /// Stores a view.
    pub fn insert(&mut self, view: DocumentView) {
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
        std::fs::create_dir_all(dir.join("nested")).expect("nested");
        std::fs::write(dir.join("nested/c.docx"), b"x").expect("write");
        let found = discover(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        let names: Vec<&str> = found.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["a.docx", "b.docx"],
            "sorted, docx only, not recursive"
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
        assert!(cache.get("missing.docx").is_none());
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
}
