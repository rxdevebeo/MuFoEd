//! Local safetensors inventory. Inspection reads only bounded JSON headers.
use crate::EditError;
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

/// Resource role, independent of its filename.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// Diffusion checkpoint.
    Model,
    /// Low-rank adapter.
    Lora,
    /// Variational autoencoder.
    Vae,
}
/// Architecture families supported by the initial diffusion adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Family {
    /// Stable Diffusion 1.5.
    Sd15,
    /// SDXL and architecturally compatible derivatives.
    Sdxl,
    /// Architecture could not be established.
    Unknown,
}
impl Family {
    /// Recommended square resolution.
    pub fn size(self) -> u32 {
        if self == Self::Sdxl {
            1024
        } else {
            512
        }
    }
    /// Classify API/metadata architecture names; unrecognized architectures stay explicit.
    pub fn parse(value: &str) -> Self {
        let value = value.to_lowercase();
        if value.contains("sdxl") || value.contains("illustrious") || value.contains("pony") {
            Self::Sdxl
        } else if value.contains("sd 1.5")
            || value.contains("sd_v1")
            || value == "sd15"
            || value == "sd1"
        {
            Self::Sd15
        } else {
            Self::Unknown
        }
    }
}
/// A local resource. Raw checkpoints need preparation before inference.
#[derive(Clone, Debug)]
pub struct Asset {
    /// Original weight file.
    pub path: PathBuf,
    /// Display name.
    pub name: String,
    /// Tensor role.
    pub kind: Kind,
    /// Detected architecture.
    pub family: Family,
    /// Prepared cache identifier, if available.
    pub ready_model: Option<String>,
}
/// Read a safetensors header with a fixed allocation cap.
pub fn header(path: &Path) -> Result<serde_json::Value, EditError> {
    let mut file = File::open(path)?;
    let mut size = [0; 8];
    file.read_exact(&mut size)?;
    let size = u64::from_le_bytes(size);
    if !(2..=16 * 1024 * 1024).contains(&size) || size + 8 > file.metadata()?.len() {
        return Err(EditError::Invalid(
            "Invalid or oversized safetensors header".into(),
        ));
    }
    let mut bytes = vec![
        0;
        usize::try_from(size).map_err(|_| EditError::Invalid(
            "Header exceeds address space".into()
        ))?
    ];
    file.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}
/// Identify tensors, without trusting extensions or naming conventions.
pub fn inspect(path: &Path) -> Result<Asset, EditError> {
    let value = header(path)?;
    let object = value
        .as_object()
        .ok_or_else(|| EditError::Invalid("Expected tensor map".into()))?;
    let keys: Vec<_> = object.keys().map(String::as_str).collect();
    let has = |s: &str| keys.iter().any(|k| k.contains(s));
    let metadata = value
        .get("__metadata__")
        .map(ToString::to_string)
        .unwrap_or_default();
    let kind = if has("lora_down.weight") || has("lora_A.weight") {
        Kind::Lora
    } else if has("diffusion_model.") || has("down_blocks.0.attentions.") {
        Kind::Model
    } else if has("encoder.conv_in.weight") && has("decoder.conv_out.weight") {
        Kind::Vae
    } else {
        return Err(EditError::Invalid(
            "Unsupported safetensors resource".into(),
        ));
    };
    let mut family = Family::parse(&metadata);
    if kind == Kind::Model {
        if has("conditioner.embedders.1.") || has("text_encoder_2.") {
            family = Family::Sdxl;
        } else if has("cond_stage_model.transformer.text_model.") {
            family = Family::Sd15;
        }
    }
    let mut name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    if let Ok(data) = std::fs::read(path.with_extension("civitai.json")) {
        if let Ok(receipt) = serde_json::from_slice::<crate::civitai::Resource>(&data) {
            let declared = Family::parse(&receipt.base_model);
            if family == Family::Unknown {
                family = declared;
            }
            name = format!("{} · {}", receipt.name, receipt.version);
        }
    }
    Ok(Asset {
        path: path.to_owned(),
        name,
        kind,
        family,
        ready_model: None,
    })
}
/// Scan a bounded directory tree, excluding tool installations and following no symlinks.
pub fn scan(root: &Path, cache: Option<&Path>) -> Result<Vec<Asset>, EditError> {
    fn visit(root: &Path, depth: usize, budget: &mut usize, out: &mut Vec<Asset>) {
        if depth > 3 || *budget == 0 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            if *budget == 0 {
                break;
            }
            *budget -= 1;
            let Ok(ty) = entry.file_type() else {
                continue;
            };
            if ty.is_symlink() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if ty.is_dir()
                && ![
                    "dependencies",
                    "venv",
                    ".venv",
                    "extensions",
                    "node_modules",
                    ".git",
                    "workspace",
                ]
                .contains(&name.as_str())
            {
                visit(&entry.path(), depth + 1, budget, out);
            } else if ty.is_file() && name.ends_with(".safetensors") {
                if let Ok(asset) = inspect(&entry.path()) {
                    out.push(asset);
                }
            }
        }
    }
    let mut out = Vec::new();
    if !root.is_dir() {
        return Err(EditError::Invalid(format!(
            "Model folder not found: {}",
            root.display()
        )));
    }
    visit(root, 0, &mut 10000, &mut out);
    if let Some(cache) = cache {
        if let Ok(bytes) = std::fs::read(cache.join("image-editor-library.json")) {
            if let Ok(paths) = serde_json::from_slice::<Vec<PathBuf>>(&bytes) {
                for path in paths.into_iter().take(4096) {
                    if !out.iter().any(|a| a.path == path) {
                        if let Ok(asset) = inspect(&path) {
                            out.push(asset);
                        }
                    }
                }
            }
        }
        for repo in std::fs::read_dir(cache).into_iter().flatten().flatten() {
            let reference = repo.path().join("refs/main");
            let Ok(revision) = std::fs::read_to_string(reference) else {
                continue;
            };
            if revision.contains(['/', '\\']) {
                continue;
            }
            let receipt = repo
                .path()
                .join("snapshots")
                .join(revision.trim())
                .join("receipt.json");
            let Ok(data) = std::fs::read(receipt) else {
                continue;
            };
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(&data) else {
                continue;
            };
            let (Some(source), Some(model)) = (value["source"].as_str(), value["repo"].as_str())
            else {
                continue;
            };
            if let Some(asset) = out.iter_mut().find(|a| a.path == Path::new(source)) {
                asset.ready_model = Some(model.to_owned());
            }
        }
    }
    out.sort_by_cached_key(|a| a.name.to_lowercase());
    Ok(out)
}

/// Remember an explicitly imported local file across editor launches.
pub fn register(path: &Path, cache: &Path) -> Result<(), EditError> {
    std::fs::create_dir_all(cache)?;
    let manifest = cache.join("image-editor-library.json");
    let mut paths: Vec<PathBuf> = std::fs::read(&manifest)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    paths.retain(|p| p.is_file());
    if !paths.iter().any(|p| p == path) {
        paths.push(path.to_owned());
    }
    let mut file = tempfile::NamedTempFile::new_in(cache)?;
    file.write_all(&serde_json::to_vec_pretty(&paths)?)?;
    file.persist(manifest).map_err(|e| EditError::Io(e.error))?;
    Ok(())
}

/// Cancellable offline checkpoint/VAE conversion process. Originals remain read-only.
pub struct Preparation {
    child: Child,
    directory: tempfile::TempDir,
    asset: Asset,
}
impl Preparation {
    /// Start offline conversion using local Python/diffusers and configs.
    pub fn start(asset: Asset, cache: &Path) -> Result<Self, EditError> {
        let directory = tempfile::tempdir()?;
        let python = std::env::var_os("IMAGE_STUDIO_PYTHON").map_or_else(
            || {
                let installed = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default())
                    .join("Programs/Python/Python311/python.exe");
                if installed.exists() {
                    installed
                } else {
                    "python".into()
                }
            },
            PathBuf::from,
        );
        let log = File::create(directory.path().join("prepare.log"))?;
        let mut command = Command::new(python);
        command
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/prepare_asset.py"))
            .arg(&asset.path)
            .arg("--kind")
            .arg(if asset.kind == Kind::Vae {
                "vae"
            } else {
                "model"
            })
            .arg("--family")
            .arg(if asset.family == Family::Sd15 {
                "sd15"
            } else {
                "sdxl"
            })
            .arg("--cache")
            .arg(cache)
            .arg("--result")
            .arg(directory.path().join("result.json"))
            .env("USE_TF", "0")
            .env("HF_HUB_OFFLINE", "1")
            .env("TRANSFORMERS_OFFLINE", "1")
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        Ok(Self {
            child: command.spawn()?,
            directory,
            asset,
        })
    }
    /// Poll for a prepared identifier or VAE path without blocking.
    pub fn poll(&mut self) -> Result<Option<(Asset, String)>, EditError> {
        let Some(status) = self.child.try_wait()? else {
            return Ok(None);
        };
        if !status.success() {
            let log = std::fs::read_to_string(self.directory.path().join("prepare.log"))
                .unwrap_or_default();
            return Err(EditError::Invalid(format!(
                "Preparation failed: {}",
                log.chars()
                    .rev()
                    .take(1800)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            )));
        }
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(self.directory.path().join("result.json"))?)?;
        Ok(Some((
            self.asset.clone(),
            value["result"]
                .as_str()
                .ok_or_else(|| EditError::Invalid("Missing preparation result".into()))?
                .to_owned(),
        )))
    }
}
impl Drop for Preparation {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inspect_tensors_without_filename_heuristics() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ambiguous.safetensors");
        let data = br#"{"lora_unet_down.lora_down.weight":{},"__metadata__":{"ss_base_model_version":"sdxl_base_v1-0"}}"#;
        std::fs::write(
            &path,
            [(data.len() as u64).to_le_bytes().as_slice(), data].concat(),
        )
        .unwrap();
        let asset = inspect(&path).unwrap();
        assert_eq!(asset.kind, Kind::Lora);
        assert_eq!(asset.family, Family::Sdxl);
        std::fs::write(&path, u64::MAX.to_le_bytes()).unwrap();
        assert!(inspect(&path).is_err());
    }
}
