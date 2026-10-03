//! Document discovery and the page cache behind the viewer.
//!
//! A document is rendered on first request and its SVG pages are then served
//! from memory. Nothing is written to disk: the viewer is a local inspection
//! tool, and a cache directory would outlive the process with stale copies of
//! documents that have since changed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use strict_ooxml::{
    ConformancePolicy, OpenOptions, RenderOptions, StrictDocument, TransitionalNormalizer,
};
use strict_ooxml_core::error::Result;

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
    use super::{discover, Cache, DocumentView, Summary};
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
