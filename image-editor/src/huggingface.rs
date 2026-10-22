//! Hugging Face Hub catalog, pinned revisions and verified local imports.
use crate::{
    assets::{self, Asset, Family, Kind},
    EditError,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
mod bundle;
mod remote;
pub use bundle::{bundle_vae, cached_bundles, diffusers_plan, download_diffusers, DiffusersPlan};
/// Public repository metadata returned by the Hub.
#[derive(Clone, Debug, Deserialize)]
pub struct Model {
    /// Repository identifier.
    pub id: String,
    /// Downloads reported by the catalog.
    #[serde(default)]
    pub downloads: u64,
    /// Architecture and library tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Private repository flag.
    #[serde(default)]
    pub private: bool,
    /// Gating policy (false, auto or manual).
    #[serde(default)]
    pub gated: serde_json::Value,
}
/// A Hub search page. The next link is validated before following it.
pub struct Page {
    /// Matching repositories.
    pub models: Vec<Model>,
    /// Official API pagination link.
    pub next: Option<String>,
}
/// File metadata pinned to a repository commit.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct File {
    /// Relative path within the repository.
    pub rfilename: String,
    /// Advertised size, when provided by the Hub.
    #[serde(default)]
    pub size: Option<u64>,
    /// Large-file integrity metadata.
    #[serde(default)]
    pub lfs: Option<Lfs>,
}
/// Hub LFS metadata; Xet-backed files also expose their content SHA256 here.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Lfs {
    /// Content SHA256.
    pub sha256: String,
    /// Content length.
    pub size: u64,
}
/// Repository details pinned to a commit, never to a moving branch during download.
#[derive(Clone, Debug, Deserialize)]
pub struct Repository {
    /// Repository identity.
    pub id: String,
    /// Immutable commit identifier.
    pub sha: String,
    /// Downloadable files.
    pub siblings: Vec<File>,
    /// Architecture/library tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Public model card metadata.
    #[serde(default, rename = "cardData")]
    pub card: serde_json::Value,
    /// Gating policy.
    #[serde(default)]
    pub gated: serde_json::Value,
}
/// Public provenance receipt, without tokens or temporary download URLs.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Receipt {
    /// Origin repository.
    pub repo: String,
    /// Pinned source revision.
    pub revision: String,
    /// Selected repository filename.
    pub file: String,
    /// Expected content SHA256.
    pub sha256: String,
    /// Requested role verified against tensor headers.
    pub kind: Kind,
    /// Architecture declared by the user/card and verified when detectable in tensors.
    pub family: Family,
}
fn invalid(message: &str) -> EditError {
    EditError::Invalid(message.into())
}
/// Accept owner/repo or its official repository URL; reject ambiguous/traversal identifiers.
pub fn repo_id(input: &str) -> Result<String, EditError> {
    let input = input.trim().trim_end_matches('/');
    let id = if input.starts_with("https://") {
        let url = reqwest::Url::parse(input)
            .map_err(|_| invalid("Invalid Hugging Face repository URL"))?;
        if url.host_str() != Some("huggingface.co")
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(invalid(
                "Use an official Hugging Face repository URL or owner/repo",
            ));
        }
        url.path().trim_matches('/').to_owned()
    } else {
        input.to_owned()
    };
    let parts: Vec<_> = id.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|s| {
            s.is_empty()
                || s.len() > 96
                || *s == "."
                || *s == ".."
                || s.starts_with('.')
                || s.ends_with('.')
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
    {
        return Err(invalid(
            "Expected owner/repo; put branch, tag or commit in Revision",
        ));
    }
    Ok(id)
}
fn revision(value: &str) -> Result<&str, EditError> {
    if value.is_empty()
        || value.len() > 160
        || value.contains("..")
        || value.chars().any(char::is_control)
        || value.contains(['\\', '?', '#'])
    {
        return Err(invalid("Invalid repository revision"));
    }
    Ok(value)
}
fn commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn url(parts: &[&str]) -> reqwest::Url {
    let mut url = reqwest::Url::parse("https://huggingface.co").expect("fixed origin");
    {
        let mut path = url.path_segments_mut().expect("hierarchical URL");
        path.clear();
        for p in parts {
            path.push(p);
        }
    }
    url
}
fn client(download: bool) -> Result<reqwest::blocking::Client, EditError> {
    reqwest::blocking::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(if download { 60 } else { 30 }))
        .user_agent("MuFoEd-ImageStudio/0.1")
        .build()
        .map_err(|_| invalid("Could not initialize Hugging Face HTTPS client"))
}
fn authorize(
    request: reqwest::blocking::RequestBuilder,
    token: &str,
) -> reqwest::blocking::RequestBuilder {
    if token.trim().is_empty() {
        request
    } else {
        request.bearer_auth(token.trim())
    }
}
fn send(
    request: reqwest::blocking::RequestBuilder,
    token: &str,
) -> Result<reqwest::blocking::Response, EditError> {
    authorize(request,token).send().and_then(reqwest::blocking::Response::error_for_status).map_err(|e| {
        EditError::Invalid(match e.status().map(|s|s.as_u16()) {
            Some(401|403)=>"Hugging Face: access denied. Check HF token and accept the model's access conditions on its page.".into(),
            Some(404)=>"Hugging Face: repository, revision or file not found (or private).".into(),
            Some(429)=>"Hugging Face rate limit reached; retry later.".into(),
            Some(s)=>format!("Hugging Face HTTP {s}"),
            None=>"Hugging Face connection failed or timed out".into(),
        })
    })
}
fn valid_next(value: &str) -> Result<reqwest::Url, EditError> {
    let url =
        reqwest::Url::parse(value).map_err(|_| invalid("Invalid Hugging Face pagination link"))?;
    if url.scheme() != "https"
        || url.host_str() != Some("huggingface.co")
        || url.path() != "/api/models"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(invalid(
            "Expected an official Hugging Face pagination endpoint",
        ));
    }
    Ok(url)
}
/// Search official model metadata; no automatic downloads or remote code execution.
pub fn search(query: &str, next: Option<&str>, token: &str) -> Result<Page, EditError> {
    let request = if let Some(next) = next {
        client(false)?.get(valid_next(next)?)
    } else {
        client(false)?.get(url(&["api", "models"])).query(&[
            ("search", query),
            ("limit", "20"),
            ("sort", "downloads"),
            ("direction", "-1"),
            ("full", "true"),
        ])
    };
    let response = send(request, token)?;
    let next = response
        .headers()
        .get(reqwest::header::LINK)
        .and_then(|h| h.to_str().ok())
        .and_then(|links| {
            links.split(',').find_map(|l| {
                if !l.contains("rel=\"next\"") {
                    return None;
                }
                l.trim()
                    .split_once('>')
                    .map(|(u, _)| u.trim_start_matches('<').to_owned())
            })
        });
    let models = serde_json::from_reader(response.take(8 * 1024 * 1024))?;
    Ok(Page { models, next })
}
/// Resolve a branch/tag to a commit and request file hashes before any weight download.
pub fn repository(
    input: &str,
    requested_revision: &str,
    token: &str,
) -> Result<Repository, EditError> {
    let id = repo_id(input)?;
    let requested_revision = revision(requested_revision.trim())?;
    let parts: Vec<_> = id.split('/').collect();
    let response = send(
        client(false)?
            .get(url(&[
                "api",
                "models",
                parts[0],
                parts[1],
                "revision",
                requested_revision,
            ]))
            .query(&[("blobs", "true")]),
        token,
    )?;
    let repo: Repository = serde_json::from_reader(response.take(16 * 1024 * 1024))?;
    if repo_id(&repo.id)? != id || !commit(&repo.sha) {
        return Err(invalid(
            "Hugging Face returned an invalid repository identity or commit",
        ));
    }
    Ok(repo)
}
/// Infer supported architecture from explicit model card and pipeline tags.
pub fn family(tags: &[String], card: &serde_json::Value) -> Family {
    let text = format!(
        "{} {}",
        tags.join(" "),
        card.get("base_model").unwrap_or(&serde_json::Value::Null)
    )
    .to_lowercase();
    if text.contains("stable-diffusion-xl")
        || text.contains("stablediffusionxl")
        || text.contains("sdxl")
        || text.contains("illustrious")
        || text.contains("pony")
    {
        Family::Sdxl
    } else if text.contains("stable-diffusion-v1")
        || text.contains("stable-diffusion-1")
        || text.contains("sd-1.5")
        || text.contains("sd_v1")
    {
        Family::Sd15
    } else {
        Family::Unknown
    }
}
impl Repository {
    /// Architecture family suggested by the public card.
    pub fn family(&self) -> Family {
        family(&self.tags, &self.card)
    }
    /// Public license identifier, when present.
    pub fn license(&self) -> &str {
        self.card
            .get("license")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("See model card")
    }
}
fn resolve(repo: &Repository, file: &File) -> Result<reqwest::Url, EditError> {
    let id = repo_id(&repo.id)?;
    if !commit(&repo.sha)
        || file.rfilename.is_empty()
        || file
            .rfilename
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == ".." || s.contains(['\\', ':']))
    {
        return Err(invalid("Invalid pinned Hugging Face file"));
    }
    let mut parts: Vec<_> = id.split('/').collect();
    parts.extend(["resolve", &repo.sha]);
    parts.extend(file.rfilename.split('/'));
    Ok(url(&parts))
}
fn stream_file(
    mut reader: impl Read,
    file: &File,
    path: &Path,
    cancel: &AtomicBool,
    progress: impl Fn(u64, Option<u64>),
) -> Result<(), EditError> {
    let total = file.lfs.as_ref().map(|f| f.size).or(file.size);
    let mut output = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| invalid("Missing download directory"))?,
    )?;
    let mut hash = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = vec![0; 131_072];
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err(invalid("Download cancelled"));
        }
        let n = reader
            .read(&mut buffer)
            .map_err(|_| invalid("Hugging Face stream failed; retry the download"))?;
        if n == 0 {
            break;
        }
        size += n as u64;
        let limit = if file.lfs.is_some() {
            32 * 1024 * 1024 * 1024_u64
        } else {
            16 * 1024 * 1024
        };
        if size > limit || total.is_some_and(|t| size > t) {
            return Err(invalid("Hugging Face file exceeds expected size or limit"));
        }
        output.write_all(&buffer[..n])?;
        hash.update(&buffer[..n]);
        progress(size, total);
    }
    if total.is_some_and(|t| t != size) {
        return Err(invalid("Truncated Hugging Face file"));
    }
    if let Some(lfs) = &file.lfs {
        if !digest(&lfs.sha256)
            || !format!("{:x}", hash.finalize()).eq_ignore_ascii_case(&lfs.sha256)
        {
            return Err(invalid("SHA256 mismatch; Hugging Face download discarded"));
        }
    }
    if cancel.load(Ordering::Acquire) {
        return Err(invalid("Download cancelled"));
    }
    output.flush()?;
    output
        .persist_noclobber(path)
        .map_err(|e| EditError::Io(e.error))?;
    Ok(())
}
/// Import one safetensors file, verifying its SHA256, tensor role and architecture before publication.
pub fn download(
    repo: &Repository,
    file: &File,
    selection: (Kind, Family),
    root: &Path,
    token: &str,
    cancel: &AtomicBool,
    progress: impl Fn(u64, Option<u64>),
) -> Result<Asset, EditError> {
    let (kind, declared) = selection;
    if cancel.load(Ordering::Acquire) {
        return Err(invalid("Download cancelled"));
    }
    let lfs = file
        .lfs
        .as_ref()
        .filter(|l| digest(&l.sha256))
        .ok_or_else(|| invalid("Missing valid SHA256 for Hugging Face weights"))?;
    if !file.rfilename.to_lowercase().ends_with(".safetensors") {
        return Err(invalid("Only safetensors weights are supported"));
    }
    let download_url = resolve(repo, file)?;
    std::fs::create_dir_all(root)?;
    let key = format!(
        "{:x}",
        Sha256::digest(format!("{}:{}:{}", repo.id, repo.sha, file.rfilename))
    );
    let destination = root.join(format!("hf-{}.safetensors", &key[..24]));
    if destination.exists() {
        return Err(invalid(
            "This revision/file is already downloaded; refresh the local library",
        ));
    }
    let stage = tempfile::tempdir_in(root)?;
    let staged = stage.path().join("weights.safetensors");
    stream_file(
        remote::reader(download_url, file, token, cancel)?,
        file,
        &staged,
        cancel,
        progress,
    )?;
    let mut asset = assets::inspect(&staged)?;
    if kind == Kind::Model {
        let header = assets::header(&staged)?;
        let full = header.as_object().is_some_and(|h| {
            h.keys().any(|k| {
                k.starts_with("conditioner.embedders.")
                    || k.starts_with("cond_stage_model.")
                    || k.starts_with("text_encoder.")
            })
        });
        if !full {
            return Err(invalid("This file is a component, not a complete SD checkpoint; use Download Diffusers for a pipeline"));
        }
    }
    if asset.kind != kind
        || (asset.family != Family::Unknown
            && declared != Family::Unknown
            && asset.family != declared)
    {
        return Err(invalid(
            "Downloaded tensors do not match the selected role or architecture",
        ));
    }
    if asset.family == Family::Unknown {
        asset.family = declared;
    }
    if asset.kind == Kind::Model && asset.family == Family::Unknown {
        return Err(invalid(
            "Choose SD 1.5 or SDXL for this checkpoint before downloading",
        ));
    }
    let receipt = Receipt {
        repo: repo.id.clone(),
        revision: repo.sha.clone(),
        file: file.rfilename.clone(),
        sha256: lfs.sha256.clone(),
        kind,
        family: asset.family,
    };
    if destination.exists() {
        return Err(invalid(
            "This revision/file is already downloaded; refresh the local library",
        ));
    }
    if cancel.load(Ordering::Acquire) {
        return Err(invalid("Download cancelled"));
    }
    std::fs::hard_link(staged, &destination)?;
    std::fs::write(
        destination.with_extension("huggingface.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    asset.path = destination;
    asset.name = format!("{} · {}", repo.id, file.rfilename);
    Ok(asset)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repository_inputs_pagination_and_pinned_file_urls_are_bounded() {
        assert_eq!(
            repo_id("https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0/").unwrap(),
            "stabilityai/stable-diffusion-xl-base-1.0"
        );
        for input in [
            "https://evil.test/owner/model",
            "owner/../model",
            "../model",
            "owner/model/resolve/main/file",
            "owner/model?token=x",
        ] {
            assert!(repo_id(input).is_err());
        }
        assert!(valid_next("https://evil.test/api/models").is_err());
        assert!(valid_next("http://huggingface.co/api/models").is_err());
        assert!(valid_next("https://huggingface.co/api/models?cursor=abc").is_ok());
        let repo = Repository {
            id: "org/model".into(),
            sha: "a".repeat(40),
            siblings: vec![],
            tags: vec![],
            card: serde_json::Value::Null,
            gated: serde_json::Value::Bool(false),
        };
        let file = File {
            rfilename: "folder/a b.safetensors".into(),
            size: None,
            lfs: None,
        };
        let url = resolve(&repo, &file).unwrap();
        assert!(url.path().ends_with("folder/a%20b.safetensors"));
        assert!(url.path().contains(&"a".repeat(40)));
        assert!(resolve(
            &repo,
            &File {
                rfilename: "../escape".into(),
                ..file
            }
        )
        .is_err());
    }
    #[test]
    fn verified_stream_is_atomic_checks_hash_size_cancel_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("weights");
        let bytes = b"verified weights";
        let file = File {
            rfilename: "a.safetensors".into(),
            size: Some(bytes.len() as u64),
            lfs: Some(Lfs {
                sha256: format!("{:x}", Sha256::digest(bytes)),
                size: bytes.len() as u64,
            }),
        };
        let cancel = AtomicBool::new(false);
        assert!(stream_file(&b"broken"[..], &file, &path, &cancel, |_, _| {}).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        cancel.store(true, Ordering::Release);
        assert!(stream_file(&bytes[..], &file, &path, &cancel, |_, _| {}).is_err());
        assert!(!path.exists());
        cancel.store(false, Ordering::Release);
        stream_file(&bytes[..], &file, &path, &cancel, |_, _| {}).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(stream_file(&bytes[..], &file, &path, &cancel, |_, _| {}).is_err());
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}
