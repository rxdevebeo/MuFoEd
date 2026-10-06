//! Resolved bundled faces and resource identity (F07 / R05).
//!
//! Layout, SVG `@font-face`, and PDF embedding must share one face program.
//! Identity is the SHA-256 of those bytes; the mapped family name alone is not
//! enough when a browser or PDF writer could pick a different installed face.
//!
//! The hash is a property of the static program, so it is computed once per
//! `(family, bold, italic)` and reused. A later call returns the same bytes
//! pointer and the same digest.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use sha2::{Digest, Sha256};

use super::builtin::{face_source, FaceSource};

/// A face chosen for measurement and delivery.
///
/// `bytes` are the exact program SVG embeds and PDF subsets. `resource_hash` is
/// their SHA-256 so callers can assert layout = SVG = PDF without comparing
/// payloads inline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedFace {
    /// Stable id: mapped family plus style flags.
    pub id: FaceId,
    /// SHA-256 of [`Self::bytes`].
    pub resource_hash: [u8; 32],
    /// Bundled family name (metric-compatible substitute).
    pub family: &'static str,
    /// Bold request the face satisfies.
    pub bold: bool,
    /// Italic request the face satisfies.
    pub italic: bool,
    /// Raw font program.
    pub bytes: &'static [u8],
    /// Variable-font `wght` instance, when the face is variable.
    pub weight: Option<f32>,
    /// Whether one face serves every `(bold, italic)` combination.
    pub single: bool,
}

/// Style key for a resolved face (Copy-friendly; no heap).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FaceId {
    /// Mapped bundled family (`Carlito`, `Cousine`, …).
    pub family: &'static str,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
}

/// SHA-256 of a face program (the resource identity contract).
#[must_use]
pub fn hash_face_bytes(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0_u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// Lower-hex form of [`hash_face_bytes`], for receipts and SVG comments.
#[must_use]
pub fn hash_face_bytes_hex(bytes: &[u8]) -> String {
    let hash = hash_face_bytes(bytes);
    let mut hex = String::with_capacity(64);
    for byte in hash {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Style key for the resolved-face cache. Weight is determined by the source.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct FaceKey {
    family: &'static str,
    bold: bool,
    italic: bool,
}

/// Resolves `(family, bold, italic)` to the bundled face layout and paint share.
///
/// Repeated calls reuse the cached [`ResolvedFace`], including its SHA-256.
/// The cache key is the mapped family plus style; variation is part of that
/// source, so a different style is a different entry.
#[must_use]
pub fn resolve_face(family: &str, bold: bool, italic: bool) -> Option<ResolvedFace> {
    let source = face_source(family, bold, italic)?;
    Some(cached_resolved_face(source, bold, italic))
}

fn cached_resolved_face(source: FaceSource, bold: bool, italic: bool) -> ResolvedFace {
    static CACHE: OnceLock<Mutex<HashMap<FaceKey, ResolvedFace>>> = OnceLock::new();
    let key = FaceKey {
        family: source.family,
        bold,
        italic,
    };
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(hit) = guard.get(&key) {
        return *hit;
    }
    let resolved = resolved_from_source(source, bold, italic);
    guard.insert(key, resolved);
    resolved
}

/// Builds a [`ResolvedFace`] from an already-looked-up [`FaceSource`].
#[must_use]
pub fn resolved_from_source(source: FaceSource, bold: bool, italic: bool) -> ResolvedFace {
    ResolvedFace {
        id: FaceId {
            family: source.family,
            bold,
            italic,
        },
        resource_hash: hash_face_bytes(source.data),
        family: source.family,
        bold,
        italic,
        bytes: source.data,
        weight: source.weight,
        single: source.single,
    }
}

impl FaceSource {
    /// SHA-256 of this face's program bytes.
    #[must_use]
    pub fn resource_hash(self) -> [u8; 32] {
        hash_face_bytes(self.data)
    }

    /// Lower-hex SHA-256 of this face's program bytes.
    #[must_use]
    pub fn resource_hash_hex(self) -> String {
        hash_face_bytes_hex(self.data)
    }
}

impl ResolvedFace {
    /// Lower-hex form of [`Self::resource_hash`].
    #[must_use]
    pub fn resource_hash_hex(self) -> String {
        hash_face_bytes_hex(self.bytes)
    }

    /// The underlying [`FaceSource`] PDF embedding already consumes.
    #[must_use]
    pub fn as_source(self) -> FaceSource {
        FaceSource {
            family: self.family,
            data: self.bytes,
            weight: self.weight,
            single: self.single,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{hash_face_bytes, resolve_face};
    use crate::font::face_source;

    #[test]
    fn resource_hash_matches_face_bytes() {
        let source = face_source("Calibri", false, false).expect("bundled");
        let resolved = resolve_face("Calibri", false, false).expect("resolved");
        assert_eq!(resolved.bytes.as_ptr(), source.data.as_ptr());
        assert_eq!(resolved.resource_hash, hash_face_bytes(source.data));
        assert_eq!(resolved.resource_hash, source.resource_hash());
        assert_eq!(resolved.family, "Carlito");
    }

    #[test]
    fn bold_and_regular_hashes_differ() {
        let regular = resolve_face("Calibri", false, false).expect("regular");
        let bold = resolve_face("Calibri", true, false).expect("bold");
        assert_ne!(regular.resource_hash, bold.resource_hash);
    }

    #[test]
    fn repeated_resolve_keeps_bytes_pointer_and_hash() {
        let first = resolve_face("Calibri", false, false).expect("first");
        let second = resolve_face("Times New Roman", false, false).expect("other");
        let again = resolve_face("Calibri", false, false).expect("again");
        assert_eq!(again.bytes.as_ptr(), first.bytes.as_ptr());
        assert_eq!(again.resource_hash, first.resource_hash);
        assert_eq!(again.resource_hash, hash_face_bytes(again.bytes));
        assert_ne!(again.resource_hash, second.resource_hash);
    }
}
