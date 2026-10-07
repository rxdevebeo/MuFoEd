//! Bounded registry for explicitly supplied, hash-pinned font assets (RTF RC02).
//!
//! Registry construction performs no filesystem or network discovery. License evidence
//! is a caller attestation, not a legal determination. Embedding/subsetting restrictions
//! in the selected face's OS/2 table are checked independently of that attestation.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use skrifa::raw::TableProvider;
use skrifa::FontRef;

use super::{
    hash_face_bytes, FaceMapping, FaceVariation, FontResource, FontResourceError,
    ResolvedFaceHandle,
};

/// Caller-attested permission for the intended SVG/PDF delivery operations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontEmbeddingPermission {
    /// No permission was supplied; the asset cannot be admitted.
    #[default]
    Forbidden,
    /// The caller has verified permission to embed and subset the supplied bytes.
    EmbedAndSubset,
}

/// Pinned metadata of one exact family/style binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontAssetManifest {
    /// Requested family this binding serves (case-insensitive, trimmed lookup).
    pub family: String,
    /// Declared family of the supplied asset; different names record a substitute.
    pub resolved_family: String,
    /// Requested bold style; an absent style binding is never synthesized.
    pub bold: bool,
    /// Requested italic style.
    pub italic: bool,
    /// Expected SHA-256 of the complete program bytes.
    pub resource_hash: [u8; 32],
    /// Face index inside the supplied program/collection.
    pub face_index: u32,
    /// Explicit variation coordinates; registry metadata is sorted by axis tag.
    pub variations: Vec<FaceVariation>,
    /// Bounded provenance reference supplied by the caller (not fetched).
    pub source: String,
    /// Caller-supplied license identifier.
    pub license_id: String,
    /// Caller-supplied license/permission evidence reference (not fetched).
    pub license_reference: String,
    /// Explicit permission attestation for both embedding and subsetting.
    pub embedding: FontEmbeddingPermission,
}

/// One explicitly supplied program and its manifest; bytes are never read from a path.
#[derive(Clone, Debug)]
pub struct FontAsset {
    /// Family/style, pinned resource identity and permission evidence.
    pub manifest: FontAssetManifest,
    /// Caller-owned program bytes, shared without copying.
    pub bytes: Arc<[u8]>,
}

/// Construction budgets, checked before hashing/parsing any program.
/// Bytes are charged per binding, even when several bindings share an Arc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontRegistryLimits {
    /// Maximum bindings (default 64; hard ceiling 1024).
    pub max_assets: usize,
    /// Maximum bytes in one binding (default 32 MiB; hard ceiling 128 MiB).
    pub max_asset_bytes: usize,
    /// Maximum sum of binding bytes (default 128 MiB; hard ceiling 1 GiB).
    pub max_total_bytes: usize,
    /// Maximum UTF-8 bytes in each provenance/license field (default 4096; ceiling 16384).
    pub max_metadata_bytes: usize,
    /// Maximum explicit axes per binding (default 32; ceiling 64).
    pub max_variations: usize,
}

impl Default for FontRegistryLimits {
    fn default() -> Self {
        Self {
            max_assets: 64,
            max_asset_bytes: 32 * 1024 * 1024,
            max_total_bytes: 128 * 1024 * 1024,
            max_metadata_bytes: 4096,
            max_variations: 32,
        }
    }
}

/// Why an explicit asset was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontAssetRejection {
    /// A declared budget was exceeded before program parsing.
    LimitExceeded,
    /// Empty, control-containing or oversized family/provenance/license metadata.
    InvalidMetadata,
    /// Another asset binds the same normalized family and exact style.
    DuplicateBinding,
    /// No explicit embedding/subsetting permission was attested.
    PermissionMissing,
    /// Program bytes differ from their pinned hash.
    HashMismatch,
    /// Selected face's OS/2 metadata is unavailable or forbids intended delivery.
    EmbeddingRestricted,
    /// Selected program/index/variation is invalid.
    InvalidResource(FontResourceError),
}

/// Transactional registry construction failure; no partial registry is returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontRegistryError {
    /// Budgets are zero or exceed documented hard ceilings.
    InvalidLimits,
    /// Asset count exceeds the configured bound.
    TooManyAssets,
    /// Asset at the supplied input index was rejected for this reason.
    Asset {
        /// Zero-based input asset index.
        index: usize,
        /// Specific rejected invariant.
        reason: FontAssetRejection,
    },
}

impl std::fmt::Display for FontRegistryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLimits => formatter.write_str("invalid font registry limits"),
            Self::TooManyAssets => formatter.write_str("font registry asset count exceeds limit"),
            Self::Asset { index, reason } => {
                write!(formatter, "font asset {index} rejected: {reason:?}")
            }
        }
    }
}
impl std::error::Error for FontRegistryError {}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct BindingKey {
    family: String,
    bold: bool,
    italic: bool,
}

#[derive(Clone, Debug)]
struct Binding {
    manifest: FontAssetManifest,
    resource: FontResource,
}

/// Immutable, cheaply cloned registry of explicit font programs.
/// Lookup only returns exact family/style bindings; it never searches the machine,
/// synthesizes an absent style, or silently selects a bundled fallback.
#[derive(Clone, Debug, Default)]
pub struct FontAssetRegistry {
    bindings: Arc<BTreeMap<BindingKey, Binding>>,
}

impl FontAssetRegistry {
    /// Validates all budgets/manifests, then pins and parses every admitted program.
    /// Input order cannot change successful lookup or manifest enumeration order.
    pub fn from_assets(
        assets: Vec<FontAsset>,
        limits: FontRegistryLimits,
    ) -> Result<Self, FontRegistryError> {
        if limits.max_assets == 0
            || limits.max_assets > 1024
            || limits.max_asset_bytes == 0
            || limits.max_asset_bytes > 128 * 1024 * 1024
            || limits.max_total_bytes == 0
            || limits.max_total_bytes > 1024 * 1024 * 1024
            || limits.max_metadata_bytes == 0
            || limits.max_metadata_bytes > 16384
            || limits.max_variations == 0
            || limits.max_variations > 64
        {
            return Err(FontRegistryError::InvalidLimits);
        }
        if assets.len() > limits.max_assets {
            return Err(FontRegistryError::TooManyAssets);
        }
        let mut total = 0usize;
        let mut keys = BTreeSet::new();
        // Preflight the complete manifest before any expensive hashing/parsing.
        for (index, asset) in assets.iter().enumerate() {
            let reject = |reason| FontRegistryError::Asset { index, reason };
            total = total
                .checked_add(asset.bytes.len())
                .ok_or_else(|| reject(FontAssetRejection::LimitExceeded))?;
            if asset.bytes.len() > limits.max_asset_bytes
                || total > limits.max_total_bytes
                || asset.manifest.variations.len() > limits.max_variations
            {
                return Err(reject(FontAssetRejection::LimitExceeded));
            }
            let metadata = &asset.manifest;
            if !valid_metadata(&metadata.family, 256)
                || !valid_metadata(&metadata.resolved_family, 256)
                || !valid_metadata(&metadata.source, limits.max_metadata_bytes)
                || !valid_metadata(&metadata.license_id, limits.max_metadata_bytes)
                || !valid_metadata(&metadata.license_reference, limits.max_metadata_bytes)
            {
                return Err(reject(FontAssetRejection::InvalidMetadata));
            }
            if metadata.embedding != FontEmbeddingPermission::EmbedAndSubset {
                return Err(reject(FontAssetRejection::PermissionMissing));
            }
            let key = binding_key(&metadata.family, metadata.bold, metadata.italic);
            if !keys.insert(key) {
                return Err(reject(FontAssetRejection::DuplicateBinding));
            }
        }
        let mut bindings = BTreeMap::new();
        for (index, asset) in assets.into_iter().enumerate() {
            let reject = |reason| FontRegistryError::Asset { index, reason };
            if hash_face_bytes(&asset.bytes) != asset.manifest.resource_hash {
                return Err(reject(FontAssetRejection::HashMismatch));
            }
            let mut manifest = asset.manifest;
            let resource = FontResource::from_shared(
                asset.bytes,
                manifest.face_index,
                manifest.variations.clone(),
            )
            .map_err(|error| reject(FontAssetRejection::InvalidResource(error)))?;
            if resource.metrics().is_none() {
                return Err(reject(FontAssetRejection::InvalidResource(
                    FontResourceError::InvalidFace,
                )));
            }
            let font =
                FontRef::from_index(resource.bytes(), manifest.face_index).map_err(|_| {
                    reject(FontAssetRejection::InvalidResource(
                        FontResourceError::InvalidFace,
                    ))
                })?;
            let os2 = font
                .os2()
                .map_err(|_| reject(FontAssetRejection::EmbeddingRestricted))?;
            let flags = os2.fs_type();
            // Restricted embedding, no subsetting, bitmap-only, unknown bits.
            if flags & (0x0002 | 0x0100 | 0x0200) != 0 || flags & !0x030E != 0 {
                return Err(reject(FontAssetRejection::EmbeddingRestricted));
            }
            trim_metadata(&mut manifest.family);
            trim_metadata(&mut manifest.resolved_family);
            manifest.variations.clone_from(&resource.key().variations);
            let key = binding_key(&manifest.family, manifest.bold, manifest.italic);
            bindings.insert(key, Binding { manifest, resource });
        }
        Ok(Self {
            bindings: Arc::new(bindings),
        })
    }

    /// Number of exact family/style bindings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Whether no assets are bound.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Deterministic manifest order by normalized family, then requested style.
    pub fn manifests(&self) -> impl ExactSizeIterator<Item = &FontAssetManifest> {
        self.bindings.values().map(|binding| &binding.manifest)
    }

    /// Resolves an explicit binding while preserving the original family request.
    /// The returned resource survives dropping the registry or original bytes owner.
    #[must_use]
    pub fn resolve(&self, family: &str, bold: bool, italic: bool) -> Option<ResolvedFaceHandle> {
        // Bound lookup allocations as well as construction metadata.
        if !valid_metadata(family, 256) {
            return None;
        }
        let binding = self.bindings.get(&binding_key(family, bold, italic))?;
        let metadata = &binding.manifest;
        let mapping = if metadata
            .family
            .eq_ignore_ascii_case(&metadata.resolved_family)
        {
            FaceMapping::ExactFamily
        } else {
            FaceMapping::Substitute
        };
        Some(ResolvedFaceHandle {
            requested_family: Arc::from(family),
            family: Arc::from(metadata.resolved_family.as_str()),
            bold,
            italic,
            mapping,
            resource: binding.resource.clone(),
        })
    }
}

fn binding_key(family: &str, bold: bool, italic: bool) -> BindingKey {
    BindingKey {
        family: family.trim().to_lowercase(),
        bold,
        italic,
    }
}

fn valid_metadata(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}

fn trim_metadata(value: &mut String) {
    let start = value.len() - value.trim_start().len();
    let end = value.trim_end().len();
    value.truncate(end);
    value.drain(..start);
}

impl PartialEq for FontAssetRegistry {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.bindings, &other.bindings)
            || self.bindings.keys().eq(other.bindings.keys())
                && self.manifests().eq(other.manifests())
    }
}
impl Eq for FontAssetRegistry {}

/// Metrics adapter for explicit assets with the existing provider as an intentional fallback.
pub struct RegistryFontProvider {
    assets: Arc<FontAssetRegistry>,
    fallback: Box<dyn super::FontProvider>,
}
impl RegistryFontProvider {
    /// Creates an adapter without discovering any other font resources.
    #[must_use]
    pub fn new(assets: Arc<FontAssetRegistry>, fallback: Box<dyn super::FontProvider>) -> Self {
        Self { assets, fallback }
    }
}
impl super::FontProvider for RegistryFontProvider {
    fn metrics(&self, family: &str, bold: bool, italic: bool) -> super::FontMetrics {
        self.assets
            .resolve(family, bold, italic)
            .and_then(|face| face.resource.metrics())
            .unwrap_or_else(|| self.fallback.metrics(family, bold, italic))
    }
    fn advance_em(&self, family: &str, ch: char, bold: bool, italic: bool) -> f64 {
        self.assets.resolve(family, bold, italic).map_or_else(
            || self.fallback.advance_em(family, ch, bold, italic),
            |face| face.resource.advance_em(ch).unwrap_or(0.0),
        )
    }
    fn has_glyph(&self, family: &str, ch: char) -> bool {
        self.assets.resolve(family, false, false).map_or_else(
            || self.fallback.has_glyph(family, ch),
            |face| face.resource.glyph_id(ch).is_some(),
        )
    }
    fn name(&self) -> &'static str {
        "explicit-font-assets"
    }
}
