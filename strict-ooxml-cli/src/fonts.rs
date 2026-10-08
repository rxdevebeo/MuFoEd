//! Bounded, explicit font-asset configuration for the render commands (RTF RC02).
//!
//! `--fonts <manifest.json>` names the exact programs to admit. The loader reads
//! only the paths the manifest declares — there is no system font discovery — pins
//! every program by SHA-256, and delegates admission (budgets, OS/2 embedding
//! flags, duplicate bindings) to [`FontAssetRegistry`]. Caller-attested permission
//! stays an attestation; the registry checks the font's own restrictions separately.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::Path;
use std::sync::Arc;

use serde::Deserialize;
use strict_ooxml::{
    FaceVariation, FontAsset, FontAssetManifest, FontAssetRegistry, FontEmbeddingPermission,
    FontRegistryLimits,
};

/// Only this manifest schema is accepted.
const SCHEMA: &str = "strictlib.font-assets.v1";
/// The one permission attestation the CLI accepts, spelled in the manifest.
const EMBED_AND_SUBSET: &str = "embed-and-subset";

#[derive(Deserialize)]
struct Manifest {
    schema: String,
    assets: Vec<ManifestAsset>,
}

#[derive(Deserialize)]
struct ManifestAsset {
    family: String,
    resolved_family: String,
    #[serde(default)]
    bold: bool,
    #[serde(default)]
    italic: bool,
    path: String,
    sha256: String,
    #[serde(default)]
    face_index: u32,
    #[serde(default)]
    variations: BTreeMap<String, f32>,
    source: String,
    license_id: String,
    license_reference: String,
    embedding: String,
}

/// Loads a manifest and builds an immutable registry from the named programs.
///
/// Paths are resolved relative to the manifest's directory. Reading is bounded by
/// [`FontRegistryLimits::default`] before any program is parsed.
pub(super) fn load(manifest_path: &str) -> Result<Arc<FontAssetRegistry>, String> {
    let bytes = std::fs::read(manifest_path)
        .map_err(|error| format!("cannot read font manifest {manifest_path}: {error}"))?;
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid font manifest {manifest_path}: {error}"))?;
    if manifest.schema != SCHEMA {
        return Err(format!("font manifest schema must be '{SCHEMA}'"));
    }
    let limits = FontRegistryLimits::default();
    if manifest.assets.is_empty() {
        return Err("font manifest declares no assets".to_owned());
    }
    if manifest.assets.len() > limits.max_assets {
        return Err(format!(
            "font manifest declares {} assets, over the {} binding budget",
            manifest.assets.len(),
            limits.max_assets
        ));
    }
    let base = Path::new(manifest_path)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut total = 0usize;
    let mut assets = Vec::with_capacity(manifest.assets.len());
    for (index, asset) in manifest.assets.into_iter().enumerate() {
        assets.push(load_asset(base, index, asset, limits, &mut total)?);
    }
    let registry = FontAssetRegistry::from_assets(assets, limits)
        .map_err(|error| format!("font manifest rejected: {error}"))?;
    Ok(Arc::new(registry))
}

fn load_asset(
    base: &Path,
    index: usize,
    asset: ManifestAsset,
    limits: FontRegistryLimits,
    total: &mut usize,
) -> Result<FontAsset, String> {
    let label = format!("font asset {index} ({})", asset.family);
    if asset.embedding != EMBED_AND_SUBSET {
        return Err(format!("{label} is not attested '{EMBED_AND_SUBSET}'"));
    }
    let hash = decode_sha256(&asset.sha256)
        .ok_or_else(|| format!("{label} has a malformed sha256 (need 64 hex digits)"))?;
    let path = base.join(&asset.path);
    let file = std::fs::File::open(&path)
        .map_err(|error| format!("{label} cannot open {}: {error}", path.display()))?;
    let cap = u64::try_from(limits.max_asset_bytes).unwrap_or(u64::MAX);
    let mut bytes = Vec::new();
    file.take(cap.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{label} cannot read {}: {error}", path.display()))?;
    if bytes.len() > limits.max_asset_bytes {
        return Err(format!(
            "{label} exceeds the {} byte per-asset budget",
            limits.max_asset_bytes
        ));
    }
    if super::sha256_hex(&bytes) != hex(hash) {
        return Err(format!("{label} does not match its pinned sha256"));
    }
    *total = total
        .checked_add(bytes.len())
        .ok_or_else(|| format!("{label} byte total overflow"))?;
    if *total > limits.max_total_bytes {
        return Err(format!(
            "font assets exceed the {} byte total budget",
            limits.max_total_bytes
        ));
    }
    Ok(FontAsset {
        bytes: Arc::from(bytes),
        manifest: FontAssetManifest {
            family: asset.family,
            resolved_family: asset.resolved_family,
            bold: asset.bold,
            italic: asset.italic,
            resource_hash: hash,
            face_index: asset.face_index,
            variations: decode_variations(index, asset.variations)?,
            source: asset.source,
            license_id: asset.license_id,
            license_reference: asset.license_reference,
            embedding: FontEmbeddingPermission::EmbedAndSubset,
        },
    })
}

fn decode_variations(
    index: usize,
    variations: BTreeMap<String, f32>,
) -> Result<Vec<FaceVariation>, String> {
    let mut out = Vec::with_capacity(variations.len());
    for (tag, value) in variations {
        let bytes = <[u8; 4]>::try_from(tag.as_bytes()).map_err(|_| {
            format!("font asset {index} variation axis '{tag}' must be 4 ASCII bytes")
        })?;
        let coordinate = FaceVariation::new(bytes, value)
            .ok_or_else(|| format!("font asset {index} variation axis '{tag}' must be finite"))?;
        out.push(coordinate);
    }
    Ok(out)
}

fn decode_sha256(value: &str) -> Option<[u8; 32]> {
    let value = value.as_bytes();
    if value.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        let high = hex_digit(value[index * 2])?;
        let low = hex_digit(value[index * 2 + 1])?;
        *byte = (high << 4) | low;
    }
    Some(out)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn hex(bytes: [u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(64);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CJK: &str = include_str!("../../strict-ooxml-render-svg/fonts/witnesses/manifest.json");
    const CJK_SHA: &str = "226a61ed488949f0809ff9961e722274464a2b50297aa80a40ab07358f974d67";
    const DEVANAGARI_SHA: &str = "16431b8a3ccaa45038cab3824008d628668f79e518ccee82f9b312022cc6d1f4";

    fn witness_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../strict-ooxml-render-svg/fonts/witnesses")
    }

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "strictlib-cli-fonts-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn manifest(sha: &str, embedding: &str) -> String {
        format!(
            r#"{{
  "schema": "strictlib.font-assets.v1",
  "assets": [
    {{
      "family": "StrictLib Witness CJK SC",
      "resolved_family": "StrictLib Witness CJK SC",
      "path": "cjk.otf",
      "sha256": "{sha}",
      "source": "pinned witness",
      "license_id": "OFL-1.1",
      "license_reference": "CJK-OFL.txt",
      "embedding": "{embedding}"
    }}
  ]
}}"#
        )
    }

    #[test]
    fn admitted_manifest_builds_a_registry() {
        let dir = temp_dir();
        std::fs::copy(witness_path().join("cjk.otf"), dir.join("cjk.otf")).unwrap();
        std::fs::write(dir.join("fonts.json"), manifest(CJK_SHA, EMBED_AND_SUBSET)).unwrap();
        let registry = load(dir.join("fonts.json").to_str().unwrap()).unwrap();
        assert_eq!(registry.len(), 1);
        assert!(registry
            .resolve("StrictLib Witness CJK SC", false, false)
            .is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn hash_mismatch_and_missing_permission_are_rejected() {
        let dir = temp_dir();
        std::fs::copy(witness_path().join("cjk.otf"), dir.join("cjk.otf")).unwrap();
        let bad_hash = dir.join("bad-hash.json");
        std::fs::write(
            &bad_hash,
            manifest(CJK_SHA, EMBED_AND_SUBSET).replace(CJK_SHA, &"0".repeat(64)),
        )
        .unwrap();
        assert!(load(bad_hash.to_str().unwrap())
            .unwrap_err()
            .contains("pinned sha256"));
        let bad_permission = dir.join("bad-permission.json");
        std::fs::write(
            &bad_permission,
            manifest(CJK_SHA, "forbidden").replace("pinned witness", "x"),
        )
        .unwrap();
        assert!(load(bad_permission.to_str().unwrap())
            .unwrap_err()
            .contains("embed-and-subset"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn malformed_sha_and_unknown_schema_are_rejected() {
        let dir = temp_dir();
        std::fs::copy(witness_path().join("cjk.otf"), dir.join("cjk.otf")).unwrap();
        let malformed = dir.join("malformed.json");
        std::fs::write(&malformed, manifest("abcd", EMBED_AND_SUBSET)).unwrap();
        assert!(load(malformed.to_str().unwrap())
            .unwrap_err()
            .contains("malformed sha256"));
        let schema = dir.join("schema.json");
        std::fs::write(
            &schema,
            manifest(CJK_SHA, EMBED_AND_SUBSET).replace(SCHEMA, "strictlib.other.v1"),
        )
        .unwrap();
        assert!(load(schema.to_str().unwrap())
            .unwrap_err()
            .contains("schema must be"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn witness_sha_constants_match_the_pinned_manifest() {
        assert!(CJK.contains(CJK_SHA));
        assert!(CJK.contains(DEVANAGARI_SHA));
    }
}
