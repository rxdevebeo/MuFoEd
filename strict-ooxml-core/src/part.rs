//! Part identifiers, part metadata and lazy part access.
//!
//! Hosts [`PartId`], [`Part`], [`Compression`] and the [`PartSource`] trait used
//! to open a package part as a streaming reader without materializing it in
//! memory (`TZ-STRICT-OOXML-RUST.md` §8.1, §8.3; stage tasks S1.4 and S1.8).

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
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
///
/// `Eq`/`Hash`/`Ord` compare the path ASCII case-insensitively (AUD-24: OPC,
/// ECMA-376 Part 2 §10.1.2.1, requires part names to be compared without
/// regard to ASCII case, so `/word/document.xml` and `/WORD/DOCUMENT.XML` are
/// the same part). `Display`/[`Self::as_str`] always return the spelling the
/// value was constructed with — two equal `PartId`s can still print
/// differently, which is intentional: it is what lets a `HashMap<PartId, _>`
/// key or a ZIP central-directory entry be found under any casing while still
/// reporting the one spelling that actually exists.
#[derive(Clone, Debug)]
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

    /// Returns the part path, for example `/word/document.xml`, in the exact
    /// spelling this value was constructed with.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq for PartId {
    /// ASCII case-insensitive (AUD-24): bytes outside `A..=Z` compare as-is.
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Eq for PartId {}

impl Hash for PartId {
    /// Hashes the ASCII-lowercased bytes, so values that compare equal always
    /// hash equal — required by `Hash`'s contract and by every
    /// `HashMap<PartId, _>`/`HashSet<PartId>` in this crate.
    fn hash<H: Hasher>(&self, state: &mut H) {
        for byte in self.0.bytes() {
            state.write_u8(byte.to_ascii_lowercase());
        }
        // A terminator byte outside the ascii-lowercase range keeps
        // concatenation-adjacent values (there are none today, since a
        // `PartId` is hashed whole, never segment-by-segment) from colliding,
        // mirroring how the standard library hashes `str`/slices.
        state.write_u8(0xff);
    }
}

impl PartialOrd for PartId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PartId {
    /// ASCII case-insensitive, consistent with [`PartialEq`] and [`Hash`]
    /// above: two spellings of the same part always compare `Equal`.
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .bytes()
            .map(|b| b.to_ascii_lowercase())
            .cmp(other.0.bytes().map(|b| b.to_ascii_lowercase()))
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
    use std::collections::hash_map::DefaultHasher;
    use std::collections::{HashMap, HashSet};
    use std::hash::{Hash, Hasher};

    fn hash_of(id: &PartId) -> u64 {
        let mut hasher = DefaultHasher::new();
        id.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn part_id_display_and_as_ref() {
        let id = PartId::new("/word/document.xml");
        assert_eq!(id.to_string(), "/word/document.xml");
        assert_eq!(id.as_str(), "/word/document.xml");
        assert_eq!(AsRef::<str>::as_ref(&id), "/word/document.xml");
    }

    /// AUD-24: `/word/document.xml` and `/WORD/DOCUMENT.XML` are the same OPC
    /// part (ECMA-376 Part 2 §10.1.2.1), so they must be `Eq`, hash equal, and
    /// compare `Equal` under `Ord` — but each still *displays* the spelling it
    /// was built with.
    #[test]
    fn eq_hash_and_ord_are_ascii_case_insensitive() {
        let lower = PartId::new("/word/document.xml");
        let upper = PartId::new("/WORD/DOCUMENT.XML");
        let mixed = PartId::new("/Word/Document.xml");

        assert_eq!(lower, upper);
        assert_eq!(lower, mixed);
        assert_eq!(hash_of(&lower), hash_of(&upper));
        assert_eq!(hash_of(&lower), hash_of(&mixed));
        assert_eq!(lower.cmp(&upper), std::cmp::Ordering::Equal);

        // Display/as_str keep the original spelling: equality does not erase it.
        assert_eq!(lower.as_str(), "/word/document.xml");
        assert_eq!(upper.as_str(), "/WORD/DOCUMENT.XML");
        assert_eq!(mixed.as_str(), "/Word/Document.xml");
    }

    /// Bytes outside ASCII `A..=Z`/`a..=z` are compared as-is: this is ASCII
    /// case-insensitivity, not a locale-aware or Unicode fold.
    #[test]
    fn non_ascii_bytes_are_compared_exactly() {
        let a = PartId::new("/wörd/document.xml");
        let b = PartId::new("/wörD/document.xml");
        let c = PartId::new("/wÖrd/document.xml");
        assert_eq!(a, b, "only the ASCII letters differ in case");
        assert_ne!(a, c, "the non-ASCII byte differs and is not folded");
    }

    /// A `HashSet`/`HashMap` keyed by `PartId` treats two spellings of the
    /// same part as one key — this is what makes the ZIP reader's duplicate
    /// check (and every part lookup) case-insensitive for free.
    #[test]
    fn case_variants_collide_in_hash_collections() {
        let mut set = HashSet::new();
        assert!(set.insert(PartId::new("/word/document.xml")));
        assert!(!set.insert(PartId::new("/WORD/DOCUMENT.XML")));
        assert_eq!(set.len(), 1);

        let mut map = HashMap::new();
        map.insert(PartId::new("/word/styles.xml"), 1);
        assert_eq!(map.get(&PartId::new("/Word/Styles.xml")), Some(&1));
    }

    #[test]
    fn compression_variants_differ() {
        assert_eq!(Compression::Stored, Compression::Stored);
        assert_ne!(Compression::Stored, Compression::Deflate);
    }
}
