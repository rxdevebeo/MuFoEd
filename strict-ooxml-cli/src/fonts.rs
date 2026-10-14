//! The fonts the render commands use besides the bundled faces (RTF RC02).
//!
//! `--fonts <manifest.json>` names exact programs: the loader reads only the
//! paths the manifest declares, pins each by SHA-256 and hands them to
//! [`FontAssets`]. `--system-fonts` adds the installed fonts the document
//! names (a manifest's binding wins over an installed one). `--strict-fonts`
//! refuses a rendering in which a font the document names is not available
//! ([`FontPolicy::Reject`]). The set used is printed with its hash.

use std::io::Read as _;
use std::path::Path;
use std::sync::Arc;

use serde::Deserialize;
use strict_ooxml::{
    system_fonts, FontAsset, FontAssetLimits, FontAssets, FontPolicy, RenderOptions, StrictDocument,
};

/// The manifest schema, as RC02 defined it.
const SCHEMA: &str = "strictlib.font-assets.v1";

#[derive(Deserialize)]
struct Manifest {
    schema: String,
    assets: Vec<ManifestAsset>,
}

/// One binding. Provenance fields (`source`, `license_id`,
/// `license_reference`, `embedding`, `resolved_family`) are accepted and not
/// interpreted: the font's own embedding flags are what the renderer checks.
#[derive(Deserialize)]
#[allow(dead_code)] // The provenance fields are read from the file, not used.
struct ManifestAsset {
    family: String,
    #[serde(default)]
    bold: bool,
    #[serde(default)]
    italic: bool,
    path: String,
    sha256: String,
    #[serde(default)]
    face_index: u32,
    #[serde(default)]
    resolved_family: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    license_id: Option<String>,
    #[serde(default)]
    license_reference: Option<String>,
    #[serde(default)]
    embedding: Option<String>,
}

/// The font options of a render command.
#[derive(Default)]
pub(super) struct FontArgs {
    manifest: Option<String>,
    system: bool,
    strict: bool,
}

impl FontArgs {
    /// Takes `--fonts <path>` or `--strict-fonts` at `args[*index]`; whether
    /// the argument was one of them.
    pub(super) fn take(&mut self, args: &[String], index: &mut usize) -> Result<bool, String> {
        match args.get(*index).map(String::as_str) {
            Some("--fonts") => {
                *index += 1;
                self.manifest = Some(
                    args.get(*index)
                        .ok_or("'--fonts' requires a manifest path")?
                        .clone(),
                );
                Ok(true)
            }
            Some("--strict-fonts") => {
                self.strict = true;
                Ok(true)
            }
            Some("--system-fonts") => {
                self.system = true;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Whether any font option was given.
    pub(super) fn is_set(&self) -> bool {
        self.manifest.is_some() || self.system || self.strict
    }

    /// `options` with the fonts and the policy these arguments ask for, for
    /// `document`.
    pub(super) fn apply(
        &self,
        mut options: RenderOptions,
        document: &StrictDocument,
    ) -> Result<RenderOptions, String> {
        let limits = FontAssetLimits::default();
        let mut assets = match &self.manifest {
            Some(manifest) => load(manifest)?,
            None => Vec::new(),
        };
        if self.system {
            let named = system_fonts::document_families(document.document());
            let installed = system_fonts::assets_for(&named, system_fonts::index(), limits)
                .map_err(|error| format!("installed fonts: {error}"))?;
            let pinned: Vec<_> = assets.iter().map(FontAsset::key).collect();
            assets.extend(
                installed
                    .into_iter()
                    .filter(|asset| !pinned.contains(&asset.key())),
            );
        }
        if !assets.is_empty() {
            let set = FontAssets::new(assets, limits)
                .map_err(|error| format!("fonts rejected: {error}"))?;
            let hash = set
                .set_hash()
                .iter()
                .fold(String::with_capacity(64), |mut hex, byte| {
                    use std::fmt::Write as _;
                    let _ = write!(hex, "{byte:02x}");
                    hex
                });
            eprintln!(
                "fonts: {} program(s) [{}], set sha256 {hash}",
                set.len(),
                set.families().join(", ")
            );
            options = options.font_assets(Arc::new(set));
        }
        if self.strict {
            options = options.font_policy(FontPolicy::Reject);
        }
        Ok(options)
    }
}

/// Loads a manifest and the programs it names; paths are relative to the
/// manifest's directory.
fn load(manifest_path: &str) -> Result<Vec<FontAsset>, String> {
    let bytes = std::fs::read(manifest_path)
        .map_err(|error| format!("cannot read font manifest {manifest_path}: {error}"))?;
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid font manifest {manifest_path}: {error}"))?;
    if manifest.schema != SCHEMA {
        return Err(format!("font manifest schema must be '{SCHEMA}'"));
    }
    if manifest.assets.is_empty() {
        return Err("font manifest declares no assets".to_owned());
    }
    let limits = FontAssetLimits::default();
    if manifest.assets.len() > limits.max_assets {
        return Err(format!(
            "font manifest declares {} assets, over the limit of {}",
            manifest.assets.len(),
            limits.max_assets
        ));
    }
    let base = Path::new(manifest_path)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut assets = Vec::with_capacity(manifest.assets.len());
    for (index, asset) in manifest.assets.into_iter().enumerate() {
        let label = format!("font asset {index} ({})", asset.family);
        let pin = decode_sha256(&asset.sha256)
            .ok_or_else(|| format!("{label} has a malformed sha256 (need 64 hex digits)"))?;
        let path = base.join(&asset.path);
        let file = std::fs::File::open(&path)
            .map_err(|error| format!("{label} cannot open {}: {error}", path.display()))?;
        let cap = u64::try_from(limits.max_asset_bytes).unwrap_or(u64::MAX);
        let mut bytes = Vec::new();
        file.take(cap.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| format!("{label} cannot read {}: {error}", path.display()))?;
        assets.push(
            FontAsset::new(&asset.family, asset.bold, asset.italic, bytes)
                .face_index(asset.face_index)
                .pinned(pin),
        );
    }
    Ok(assets)
}

fn decode_sha256(text: &str) -> Option<[u8; 32]> {
    let text = text.trim();
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (byte, pair) in out.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::decode_sha256;

    #[test]
    fn sha256_pins_are_64_hex_digits() {
        assert!(decode_sha256(&"ab".repeat(32)).is_some());
        assert!(decode_sha256("abc").is_none());
        assert!(decode_sha256(&"zz".repeat(32)).is_none());
    }
}
