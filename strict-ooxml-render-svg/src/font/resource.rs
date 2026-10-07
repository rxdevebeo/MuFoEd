//! Owned/shared font programs and canonical instance identity (RTF RC02).
//!
//! This contract does not discover installed fonts or admit assets into renders.
//! The legacy static face remains available; its adapter borrows bundled bytes.

use std::sync::Arc;

use skrifa::prelude::{MetadataProvider, Tag};
use skrifa::FontRef;

use super::{hash_face_bytes, resolve_face, ResolvedFace};

/// An OpenType design coordinate. Finite values only; negative zero is canonicalized.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FaceVariation {
    tag: [u8; 4],
    value_bits: u32,
}

impl FaceVariation {
    /// Creates a coordinate, rejecting NaN and infinity.
    #[must_use]
    pub fn new(tag: [u8; 4], value: f32) -> Option<Self> {
        value.is_finite().then(|| Self {
            tag,
            value_bits: if value == 0.0 { 0 } else { value.to_bits() },
        })
    }

    /// OpenType axis tag.
    #[must_use]
    pub fn tag(self) -> [u8; 4] {
        self.tag
    }

    /// Coordinate in design units.
    #[must_use]
    pub fn value(self) -> f32 {
        f32::from_bits(self.value_bits)
    }
}

/// Identity of the exact program and selected instance; independent of family aliases.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FaceInstanceKey {
    /// SHA-256 of the entire font program (including a collection, when present).
    pub resource_hash: [u8; 32],
    /// Face index inside that program.
    pub face_index: u32,
    /// Explicit design coordinates sorted by tag, with no duplicate axes.
    pub variations: Vec<FaceVariation>,
}

#[derive(Clone, Debug)]
enum ProgramBytes {
    Bundled(&'static [u8]),
    Shared(Arc<[u8]>),
}

/// A validated font program whose bytes remain alive while consumers hold it.
#[derive(Clone, Debug)]
pub struct FontResource {
    bytes: ProgramBytes,
    key: FaceInstanceKey,
}

/// Failure to construct a font resource. No silent face/axis fallback is performed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontResourceError {
    /// Font bytes or requested collection index cannot be parsed.
    InvalidFace,
    /// More than one coordinate was supplied for the same axis.
    DuplicateAxis,
    /// The selected face has no requested axis, or its coordinate is out of range.
    InvalidVariation,
}

impl std::fmt::Display for FontResourceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidFace => "invalid font program or face index",
            Self::DuplicateAxis => "duplicate font variation axis",
            Self::InvalidVariation => "unsupported or out-of-range font variation",
        })
    }
}
impl std::error::Error for FontResourceError {}

impl FontResource {
    /// Accepts explicitly supplied shared bytes and validates the selected face/axes.
    /// Asset licensing and renderer admission belong to the future registry.
    pub fn from_shared(
        bytes: Arc<[u8]>,
        face_index: u32,
        mut variations: Vec<FaceVariation>,
    ) -> Result<Self, FontResourceError> {
        let font =
            FontRef::from_index(&bytes, face_index).map_err(|_| FontResourceError::InvalidFace)?;
        variations.sort_by_key(|coordinate| coordinate.tag);
        if variations.windows(2).any(|pair| pair[0].tag == pair[1].tag) {
            return Err(FontResourceError::DuplicateAxis);
        }
        for coordinate in &variations {
            let axis = font
                .axes()
                .get_by_tag(Tag::new(&coordinate.tag))
                .ok_or(FontResourceError::InvalidVariation)?;
            if coordinate.value() < axis.min_value() || coordinate.value() > axis.max_value() {
                return Err(FontResourceError::InvalidVariation);
            }
        }
        let key = FaceInstanceKey {
            resource_hash: hash_face_bytes(&bytes),
            face_index,
            variations,
        };
        Ok(Self {
            bytes: ProgramBytes::Shared(bytes),
            key,
        })
    }

    /// Adapts a legacy bundled face without copying its font program.
    #[must_use]
    pub fn from_bundled(face: ResolvedFace) -> Self {
        Self {
            bytes: ProgramBytes::Bundled(face.bytes),
            key: face.instance_key(),
        }
    }

    /// The exact font program to measure, shape and embed.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        match &self.bytes {
            ProgramBytes::Bundled(bytes) => bytes,
            ProgramBytes::Shared(bytes) => bytes,
        }
    }

    /// Canonical program/instance identity.
    #[must_use]
    pub fn key(&self) -> &FaceInstanceKey {
        &self.key
    }
}

/// Family mapping provenance. Exact family does not assert glyph/script coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceMapping {
    /// Requested name identifies the bundled family itself.
    ExactFamily,
    /// Known family alias mapped to a bundled substitute.
    Substitute,
    /// Unknown family uses the deterministic default.
    DefaultFallback,
}

/// Shared-byte counterpart of the legacy Copy face, retaining request provenance.
#[derive(Clone, Debug)]
pub struct ResolvedFaceHandle {
    /// Original family request, before mapping.
    pub requested_family: Arc<str>,
    /// Selected family, separate from program identity.
    pub family: Arc<str>,
    /// Requested bold style (not a claim that synthetic style was applied).
    pub bold: bool,
    /// Requested italic style.
    pub italic: bool,
    /// Family mapping provenance.
    pub mapping: FaceMapping,
    /// Exact program/instance shared by future consumers.
    pub resource: FontResource,
}

/// Resolves the bundled provider into the shared-byte contract, preserving its output.
#[must_use]
pub fn resolve_bundled_handle(
    family: &str,
    bold: bool,
    italic: bool,
) -> Option<ResolvedFaceHandle> {
    let face = resolve_face(family, bold, italic)?;
    let requested = family.trim();
    let mapping = if requested.eq_ignore_ascii_case(face.family) {
        FaceMapping::ExactFamily
    } else if face.family == "Carlito" && !requested.eq_ignore_ascii_case("Calibri") {
        FaceMapping::DefaultFallback
    } else {
        FaceMapping::Substitute
    };
    Some(ResolvedFaceHandle {
        requested_family: Arc::from(family),
        family: Arc::from(face.family),
        bold,
        italic,
        mapping,
        resource: FontResource::from_bundled(face),
    })
}

impl ResolvedFace {
    /// Canonical identity used by the shared-byte adapter and shaping caches.
    #[must_use]
    pub fn instance_key(self) -> FaceInstanceKey {
        FaceInstanceKey {
            resource_hash: self.resource_hash,
            face_index: 0,
            variations: self
                .weight
                .and_then(|weight| FaceVariation::new(*b"wght", weight))
                .into_iter()
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_adapter_preserves_bytes_identity_and_provenance() {
        for (request, mapping) in [
            ("Arimo", FaceMapping::ExactFamily),
            ("ArialMT", FaceMapping::Substitute),
            ("Calibri", FaceMapping::Substitute),
            ("missing family", FaceMapping::DefaultFallback),
        ] {
            let legacy = resolve_face(request, false, false).unwrap();
            let handle = resolve_bundled_handle(request, false, false).unwrap();
            assert_eq!(handle.resource.bytes().as_ptr(), legacy.bytes.as_ptr());
            assert_eq!(handle.resource.key(), &legacy.instance_key());
            assert_eq!(handle.requested_family.as_ref(), request);
            assert_eq!(handle.mapping, mapping);
        }
    }

    #[test]
    fn shared_bytes_survive_owner_drop_and_match_bundled_instance() {
        let legacy = resolve_face("Arial", false, false).unwrap();
        let bytes: Arc<[u8]> = Arc::from(legacy.bytes);
        let resource =
            FontResource::from_shared(Arc::clone(&bytes), 0, legacy.instance_key().variations)
                .unwrap();
        assert_eq!(resource.bytes().as_ptr(), bytes.as_ptr());
        drop(bytes);
        assert_eq!(resource.key(), &legacy.instance_key());
        assert_eq!(hash_face_bytes(resource.bytes()), legacy.resource_hash);
    }

    #[test]
    fn variable_weight_is_part_of_identity_even_when_program_is_shared() {
        let regular = resolve_face("Arial", false, false).unwrap();
        let bold = resolve_face("Arial", true, false).unwrap();
        assert_eq!(regular.resource_hash, bold.resource_hash);
        assert_ne!(regular.instance_key(), bold.instance_key());
    }

    #[test]
    fn malformed_program_index_and_variations_are_rejected() {
        assert!(matches!(
            FontResource::from_shared(Arc::from(&b"invalid"[..]), 0, vec![]),
            Err(FontResourceError::InvalidFace)
        ));
        let face = resolve_face("Arial", false, false).unwrap();
        assert!(matches!(
            FontResource::from_shared(Arc::from(face.bytes), 1, vec![]),
            Err(FontResourceError::InvalidFace)
        ));
        let coordinate = FaceVariation::new(*b"wght", 400.0).unwrap();
        assert!(matches!(
            FontResource::from_shared(Arc::from(face.bytes), 0, vec![coordinate, coordinate]),
            Err(FontResourceError::DuplicateAxis)
        ));
        let unknown = FaceVariation::new(*b"xxxx", 1.0).unwrap();
        assert!(matches!(
            FontResource::from_shared(Arc::from(face.bytes), 0, vec![unknown]),
            Err(FontResourceError::InvalidVariation)
        ));
        let huge = FaceVariation::new(*b"wght", 1_000_000.0).unwrap();
        assert!(matches!(
            FontResource::from_shared(Arc::from(face.bytes), 0, vec![huge]),
            Err(FontResourceError::InvalidVariation)
        ));
        assert!(FaceVariation::new(*b"wght", f32::NAN).is_none());
        assert!(FaceVariation::new(*b"wght", f32::INFINITY).is_none());
        assert_eq!(
            FaceVariation::new(*b"slnt", -0.0),
            FaceVariation::new(*b"slnt", 0.0)
        );
    }
}
