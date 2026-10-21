//! Minimal Diffusers snapshots prepared offline for the pinned image backend.
use super::{
    assets, commit, digest, invalid, repo_id, resolve, stream_file, Asset, AtomicBool, Deserialize,
    Digest, Duration, EditError, Family, File, Kind, Ordering, Path, PathBuf, Repository,
    Serialize, Sha256, Write,
};
use std::process::{Command, Stdio};
/// Required files for a standard unsharded SD 1.5 or SDXL pipeline.
#[derive(Clone, Debug)]
pub struct DiffusersPlan {
    /// Files selected from this immutable repository revision.
    pub files: Vec<File>,
    /// Whether a tokenizer JSON must be produced offline from CLIP vocab/merges.
    pub prepare_tokenizers: bool,
    /// Estimated download size.
    pub size: u64,
    /// Relative VAE filename selected by the plan.
    pub vae: String,
}
/// Plan a minimal pipeline. Unsupported/sharded layouts are reported rather than partially downloaded.
pub fn diffusers_plan(repo: &Repository, family: Family) -> Result<DiffusersPlan, EditError> {
    if family == Family::Unknown {
        return Err(invalid("Select the SD 1.5 or SDXL architecture"));
    }
    let find = |names: &[&str]| {
        repo.siblings
            .iter()
            .find(|f| names.first() == Some(&f.rfilename.as_str()))
            .or_else(|| {
                names
                    .iter()
                    .find_map(|n| repo.siblings.iter().find(|f| &f.rfilename == n))
            })
            .cloned()
            .ok_or_else(|| {
                EditError::Invalid(format!(
                    "Diffusers component missing: {}",
                    names.join(" / ")
                ))
            })
    };
    let mut files = Vec::new();
    for path in [
        "model_index.json",
        "unet/config.json",
        "vae/config.json",
        "scheduler/scheduler_config.json",
        "text_encoder/config.json",
    ] {
        files.push(find(&[path])?);
    }
    if family == Family::Sdxl {
        files.push(find(&["text_encoder_2/config.json"])?);
    }
    for names in [
        vec![
            "unet/diffusion_pytorch_model.fp16.safetensors",
            "unet/diffusion_pytorch_model.safetensors",
        ],
        vec![
            "text_encoder/model.fp16.safetensors",
            "text_encoder/model.safetensors",
        ],
    ] {
        files.push(find(&names)?);
    }
    if family == Family::Sdxl {
        files.push(find(&[
            "text_encoder_2/model.fp16.safetensors",
            "text_encoder_2/model.safetensors",
        ])?);
    }
    let vae = find(&[
        "vae/diffusion_pytorch_model.safetensors",
        "vae/diffusion_pytorch_model.fp16.safetensors",
    ])?;
    let vae_name = vae.rfilename.clone();
    files.push(vae);
    let mut prepare = false;
    for folder in if family == Family::Sdxl {
        vec!["tokenizer", "tokenizer_2"]
    } else {
        vec!["tokenizer"]
    } {
        let fast = format!("{folder}/tokenizer.json");
        if let Ok(file) = find(&[&fast]) {
            files.push(file);
        } else {
            prepare = true;
            for name in [
                "vocab.json",
                "merges.txt",
                "tokenizer_config.json",
                "special_tokens_map.json",
            ] {
                let path = format!("{folder}/{name}");
                if let Ok(file) = find(&[&path]) {
                    files.push(file);
                } else if matches!(name, "vocab.json" | "merges.txt") {
                    return Err(invalid("CLIP tokenizer vocab/merges are missing"));
                }
            }
        }
    }
    for f in &files {
        if f.rfilename.ends_with(".safetensors")
            && f.lfs.as_ref().is_none_or(|l| !digest(&l.sha256))
        {
            return Err(invalid("Pipeline weights lack SHA256 metadata"));
        }
    }
    let size = files
        .iter()
        .map(|f| f.lfs.as_ref().map(|l| l.size).or(f.size).unwrap_or(0))
        .sum();
    Ok(DiffusersPlan {
        files,
        prepare_tokenizers: prepare,
        size,
        vae: vae_name,
    })
}
#[derive(Serialize, Deserialize)]
struct BundleReceipt {
    repo: String,
    revision: String,
    family: Family,
    alias: String,
    vae: String,
    files: Vec<String>,
}
fn json(snapshot: &Path, name: &str) -> Result<serde_json::Value, EditError> {
    Ok(serde_json::from_reader(std::fs::File::open(
        snapshot.join(name),
    )?)?)
}
fn validate_pipeline(snapshot: &Path, family: Family) -> Result<(), EditError> {
    let index = json(snapshot, "model_index.json")?;
    let class = if family == Family::Sdxl {
        "StableDiffusionXLPipeline"
    } else {
        "StableDiffusionPipeline"
    };
    if index["_class_name"] != class {
        return Err(invalid(
            "This Diffusers pipeline class is not supported (base SD 1.5/SDXL only)",
        ));
    }
    let unet = json(snapshot, "unet/config.json")?;
    let cross = if family == Family::Sdxl { 2048 } else { 768 };
    if unet["in_channels"] != 4
        || unet["cross_attention_dim"] != cross
        || unet["block_out_channels"]
            != serde_json::json!(if family == Family::Sdxl {
                vec![320, 640, 1280]
            } else {
                vec![320, 640, 1280, 1280]
            })
    {
        return Err(invalid(
            "UNet configuration is incompatible with the current image backend",
        ));
    }
    if json(snapshot, "scheduler/scheduler_config.json")?["prediction_type"]
        .as_str()
        .is_some_and(|p| p != "epsilon")
    {
        return Err(invalid(
            "The current DPM++ adapter requires epsilon prediction",
        ));
    }
    let l = json(snapshot, "text_encoder/config.json")?;
    if l["hidden_size"] != 768
        || (family == Family::Sdxl
            && json(snapshot, "text_encoder_2/config.json")?["hidden_size"] != 1280)
    {
        return Err(invalid("Unsupported CLIP encoder configuration"));
    }
    Ok(())
}
fn tokenizers(snapshot: &Path, cancel: &AtomicBool) -> Result<(), EditError> {
    let installed = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default())
        .join("Programs/Python/Python311/python.exe");
    let python = std::env::var_os("IMAGE_STUDIO_PYTHON").map_or_else(
        || {
            if installed.exists() {
                installed
            } else {
                "python".into()
            }
        },
        PathBuf::from,
    );
    let log = tempfile::tempfile()?;
    let mut command = Command::new(python);
    command
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/prepare_hf_tokenizers.py"))
        .arg(snapshot)
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
    let mut child = command.spawn()?;
    let start = std::time::Instant::now();
    loop {
        if cancel.load(Ordering::Acquire) || start.elapsed() > Duration::from_secs(90) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(invalid("Tokenizer preparation cancelled or timed out"));
        }
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(invalid("Offline CLIP tokenizer preparation failed; check IMAGE_STUDIO_PYTHON and transformers/tokenizers installation"))
            };
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
/// Download a pinned minimal pipeline and publish its local alias only after successful validation.
pub fn download_diffusers(
    repo: &Repository,
    family: Family,
    cache: &Path,
    token: &str,
    cancel: &AtomicBool,
    progress: impl Fn(u64, Option<u64>),
) -> Result<Asset, EditError> {
    let plan = diffusers_plan(repo, family)?;
    let key = format!(
        "{:x}",
        Sha256::digest(format!("{}:{}:{family:?}", repo.id, repo.sha))
    );
    let alias = format!(
        "local/hf-{}-{}",
        if family == Family::Sdxl {
            "sdxl"
        } else {
            "sd15"
        },
        &key[..24]
    );
    let root = cache.join(format!("models--{}", alias.replace('/', "--")));
    if root.join("refs/main").exists() {
        return Err(invalid(
            "This pipeline revision is already downloaded; refresh the local library",
        ));
    }
    std::fs::create_dir_all(cache)?;
    let stage = tempfile::tempdir_in(cache)?;
    let mut downloaded = 0;
    // Configs are deliberately before weights, so incompatible pipelines fail without a large download.
    for file in &plan.files {
        let path = stage.path().join(&file.rfilename);
        std::fs::create_dir_all(path.parent().expect("component path"))?;
        let response = super::remote::reader(resolve(repo, file)?, file, token, cancel)?;
        stream_file(response, file, &path, cancel, |n, _| {
            progress(downloaded + n, Some(plan.size));
        })?;
        downloaded += std::fs::metadata(&path)?.len();
        if file.rfilename == "text_encoder/config.json" && family == Family::Sd15
            || file.rfilename == "text_encoder_2/config.json" && family == Family::Sdxl
        {
            validate_pipeline(stage.path(), family)?;
        }
        if file.rfilename.ends_with(".safetensors") {
            assets::header(&path)?;
        }
    }
    if plan.prepare_tokenizers {
        tokenizers(stage.path(), cancel)?;
    }
    for name in if family == Family::Sdxl {
        vec!["tokenizer/tokenizer.json", "tokenizer_2/tokenizer.json"]
    } else {
        vec!["tokenizer/tokenizer.json"]
    } {
        let _ = json(stage.path(), name)?;
    }
    let mut published_files: Vec<_> = plan.files.iter().map(|f| f.rfilename.clone()).collect();
    for name in if family == Family::Sdxl {
        vec!["tokenizer/tokenizer.json", "tokenizer_2/tokenizer.json"]
    } else {
        vec!["tokenizer/tokenizer.json"]
    } {
        if !published_files.iter().any(|p| p == name) {
            published_files.push(name.into());
        }
    }
    let receipt = BundleReceipt {
        repo: repo.id.clone(),
        revision: repo.sha.clone(),
        family,
        alias: alias.clone(),
        vae: plan.vae,
        files: published_files,
    };
    std::fs::write(
        stage.path().join("huggingface-bundle.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    if cancel.load(Ordering::Acquire) {
        return Err(invalid("Download cancelled"));
    }
    let snapshot = root.join("snapshots").join(&repo.sha);
    std::fs::create_dir_all(root.join("snapshots"))?;
    std::fs::rename(stage.path(), &snapshot)?;
    std::fs::create_dir_all(root.join("refs"))?;
    let mut reference = tempfile::NamedTempFile::new_in(root.join("refs"))?;
    reference.write_all(repo.sha.as_bytes())?;
    reference
        .persist_noclobber(root.join("refs/main"))
        .map_err(|e| EditError::Io(e.error))?;
    Ok(Asset {
        path: snapshot.join("huggingface-bundle.json"),
        name: format!("{} · {}", repo.id, &repo.sha[..8]),
        kind: Kind::Model,
        family,
        ready_model: Some(alias),
    })
}
/// Discover only fully published Hugging Face pipeline imports.
pub fn cached_bundles(cache: &Path) -> Vec<Asset> {
    std::fs::read_dir(cache)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let revision = std::fs::read_to_string(entry.path().join("refs/main")).ok()?;
            if !commit(revision.trim()) {
                return None;
            }
            let path = entry
                .path()
                .join("snapshots")
                .join(revision.trim())
                .join("huggingface-bundle.json");
            let receipt: BundleReceipt =
                serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
            if receipt.revision != revision.trim()
                || repo_id(&receipt.repo).is_err()
                || !receipt.alias.starts_with("local/hf-")
            {
                return None;
            }
            let snapshot = path.parent()?;
            if receipt.files.iter().any(|p| {
                p.split('/')
                    .any(|s| s == ".." || s.is_empty() || s.contains(['\\', ':']))
                    || !snapshot.join(p).is_file()
            }) {
                return None;
            }
            Some(Asset {
                path,
                name: format!("{} · {}", receipt.repo, &receipt.revision[..8]),
                kind: Kind::Model,
                family: receipt.family,
                ready_model: Some(receipt.alias),
            })
        })
        .collect()
}
/// Imported native VAE, used when no explicit VAE was selected by the user.
pub fn bundle_vae(asset: &Asset) -> Option<PathBuf> {
    if asset.path.file_name()?.to_str()? != "huggingface-bundle.json" {
        return None;
    }
    let receipt: BundleReceipt = serde_json::from_slice(&std::fs::read(&asset.path).ok()?).ok()?;
    if !receipt.vae.starts_with("vae/")
        || receipt.vae.contains("..")
        || receipt.vae.contains(['\\', ':'])
    {
        return None;
    }
    Some(asset.path.parent()?.join(receipt.vae)).filter(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plan_prefers_fp16_weights_keeps_fp32_vae_and_requires_tokenizers() {
        let paths = [
            "model_index.json",
            "unet/config.json",
            "vae/config.json",
            "scheduler/scheduler_config.json",
            "text_encoder/config.json",
            "text_encoder_2/config.json",
            "unet/diffusion_pytorch_model.fp16.safetensors",
            "unet/diffusion_pytorch_model.safetensors",
            "text_encoder/model.fp16.safetensors",
            "text_encoder_2/model.fp16.safetensors",
            "vae/diffusion_pytorch_model.safetensors",
            "vae/diffusion_pytorch_model.fp16.safetensors",
            "tokenizer/vocab.json",
            "tokenizer/merges.txt",
            "tokenizer_2/tokenizer.json",
        ];
        let mut repo = Repository {
            id: "org/model".into(),
            sha: "a".repeat(40),
            siblings: paths
                .iter()
                .map(|p| File {
                    rfilename: (*p).into(),
                    size: Some(1),
                    lfs: p.ends_with(".safetensors").then(|| super::super::Lfs {
                        sha256: "b".repeat(64),
                        size: 1,
                    }),
                })
                .collect(),
            tags: vec![],
            card: serde_json::Value::Null,
            gated: serde_json::Value::Bool(false),
        };
        let plan = diffusers_plan(&repo, Family::Sdxl).unwrap();
        assert!(plan.prepare_tokenizers);
        assert!(plan
            .files
            .iter()
            .any(|f| f.rfilename == "unet/diffusion_pytorch_model.fp16.safetensors"));
        assert!(!plan
            .files
            .iter()
            .any(|f| f.rfilename == "unet/diffusion_pytorch_model.safetensors"));
        assert_eq!(plan.vae, "vae/diffusion_pytorch_model.safetensors");
        repo.siblings
            .retain(|f| f.rfilename != "tokenizer/merges.txt");
        assert!(diffusers_plan(&repo, Family::Sdxl).is_err());
    }
    #[test]
    fn incomplete_bundle_is_not_admitted_and_completed_bundle_survives_rescan() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("models--local--hf-sdxl-fixture");
        let sha = "a".repeat(40);
        let snapshot = root.join("snapshots").join(&sha);
        std::fs::create_dir_all(&snapshot).unwrap();
        std::fs::create_dir_all(root.join("refs")).unwrap();
        std::fs::write(root.join("refs/main"), &sha).unwrap();
        let receipt = BundleReceipt {
            repo: "org/model".into(),
            revision: sha,
            family: Family::Sdxl,
            alias: "local/hf-sdxl-fixture".into(),
            vae: "vae/weights.safetensors".into(),
            files: vec!["vae/weights.safetensors".into()],
        };
        std::fs::write(
            snapshot.join("huggingface-bundle.json"),
            serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();
        assert!(cached_bundles(dir.path()).is_empty());
        std::fs::create_dir(snapshot.join("vae")).unwrap();
        std::fs::write(snapshot.join("vae/weights.safetensors"), b"fixture").unwrap();
        let found = cached_bundles(dir.path());
        assert_eq!(found.len(), 1);
        assert!(bundle_vae(&found[0]).unwrap().is_file());
        assert_eq!(
            found[0].ready_model.as_deref(),
            Some("local/hf-sdxl-fixture")
        );
    }
}
