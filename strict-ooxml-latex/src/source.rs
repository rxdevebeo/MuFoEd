//! Source resolution for `\input`/`\include`.
//!
//! The core importer never touches the filesystem on its own: it asks a
//! [`SourceProvider`] for the bytes of a named file. This keeps the parser
//! deterministic and testable, and lets a caller supply an in-memory corpus or
//! a sandboxed root.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use crate::LatexError;

/// Supplies the bytes of a named LaTeX source file.
pub trait SourceProvider {
    /// Reads `name`, returning [`LatexError::SourceNotFound`] when it is absent.
    fn read(&self, name: &str) -> Result<Vec<u8>, LatexError>;

    /// Optional conversion of opaque TeX graphics or unsupported image formats.
    /// Default providers start no processes and access no network resources.
    fn render_graphic(
        &self,
        _request: &crate::RenderRequest<'_>,
    ) -> Result<Option<crate::RenderedGraphic>, LatexError> {
        Ok(None)
    }
}

/// A provider that holds every file in memory; used by tests and embedding.
#[derive(Clone, Debug, Default)]
pub struct MemorySource {
    files: BTreeMap<String, Vec<u8>>,
}

impl MemorySource {
    /// Creates an empty in-memory source.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces a file.
    pub fn insert(&mut self, name: impl Into<String>, bytes: impl Into<Vec<u8>>) {
        self.files.insert(name.into(), bytes.into());
    }
}

impl SourceProvider for MemorySource {
    fn read(&self, name: &str) -> Result<Vec<u8>, LatexError> {
        self.files
            .get(name)
            .cloned()
            .ok_or_else(|| LatexError::SourceNotFound {
                name: name.to_owned(),
            })
    }
}

/// A provider rooted at a directory; names are resolved under the root.
///
/// Absolute paths, parent components and drive prefixes are rejected, so a
/// hostile `\input{../../../etc/passwd}` cannot escape the root.
#[derive(Clone, Debug)]
pub struct FsSource {
    root: PathBuf,
}

impl FsSource {
    /// Creates a provider rooted at `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn resolve(&self, name: &str) -> Option<PathBuf> {
        let relative = Path::new(name);
        if relative.is_absolute() {
            return None;
        }
        for component in relative.components() {
            match component {
                Component::Normal(_) => {}
                _ => return None,
            }
        }
        Some(self.root.join(relative))
    }
}

impl SourceProvider for FsSource {
    fn read(&self, name: &str) -> Result<Vec<u8>, LatexError> {
        let path = self
            .resolve(name)
            .ok_or_else(|| LatexError::SourceNotFound {
                name: name.to_owned(),
            })?;
        std::fs::read(&path).map_err(|source| LatexError::Io {
            name: name.to_owned(),
            source,
        })
    }
}
