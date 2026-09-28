//! Part identifiers, part metadata and lazy part access.
//!
//! Hosts [`PartId`], [`Part`], [`Compression`] and the [`PartSource`] trait used
//! to open a package part as a streaming reader without materializing it in
//! memory (`TZ-STRICT-OOXML-RUST.md` §8.1, §8.3; stage tasks S1.4 and S1.8).

use std::fmt;
use std::io::Read;
use std::sync::Arc;

use crate::error::Result;

/// Normalized, absolute identifier of a package part, for example
/// `/word/document.xml`.
///
/// Values are cheap to clone: the path is shared through an [`Arc`]. Under
/// stage 1 only canonical, root-relative paths are valid; untrusted
/// relationship targets must be passed through the path canonicalization of
/// [`crate::opc::path`] before a `PartId` is constructed.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartId(Arc<str>);

impl PartId {
    /// Creates a part identifier from a canonical absolute path.
    ///
    /// The caller is responsible for canonicalization (see
    /// [`crate::opc::path`]); this constructor performs no validation so that it
    /// stays allocation-only and panic-free.
    pub fn new(path: impl Into<Arc<str>>) -> Self {
        Self(path.into())
    }

    /// Returns the part path, for example `/word/document.xml`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PartId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for PartId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Compression method of a package part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Compression {
    /// Stored (method 0): bytes are kept verbatim.
    Stored,
    /// Deflate (method 8): bytes are compressed with raw DEFLATE.
    Deflate,
}

/// Metadata for a single package part.
///
/// The actual bytes are not read when the package is opened; they are decoded
/// on demand through [`PartSource::open_part`] (stage task S1.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// Canonical part identifier.
    pub id: PartId,
    /// Resolved content type, if declared in `[Content_Types].xml`.
    pub content_type: Option<Arc<str>>,
    /// Size of the part as stored in the archive, in bytes.
    pub compressed_size: u64,
    /// Size of the part after decompression, in bytes.
    pub uncompressed_size: u64,
    /// Compression method used for the part.
    pub compression: Compression,
}

/// A source that can open package parts by id.
pub trait PartSource {
    /// Opens the part identified by `id` as a streaming reader.
    ///
    /// The returned reader enforces the configured per-part limits and verifies
    /// the stored CRC-32 once the part is fully consumed.
    ///
    /// If the owning package was configured with a raw normalizer, the reader
    /// yields the **normalized** bytes (which requires materializing the part);
    /// otherwise the bytes are streamed as-is.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::StrictError::MissingPart`] if no such part exists.
    fn open_part(&self, id: &PartId) -> Result<Box<dyn Read + '_>>;
}

#[cfg(test)]
mod tests {
    use super::{Compression, PartId};

    #[test]
    fn part_id_display_and_as_ref() {
        let id = PartId::new("/word/document.xml");
        assert_eq!(id.to_string(), "/word/document.xml");
        assert_eq!(id.as_str(), "/word/document.xml");
        assert_eq!(AsRef::<str>::as_ref(&id), "/word/document.xml");
    }

    #[test]
    fn compression_variants_differ() {
        assert_eq!(Compression::Stored, Compression::Stored);
        assert_ne!(Compression::Stored, Compression::Deflate);
    }
}
