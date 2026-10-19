//! Civitai public REST catalog and verified, atomic safetensors downloads.
use crate::{
    assets::{self, Asset, Family, Kind},
    EditError,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

/// One downloadable version/file. Identity and SHA256 are retained alongside the file.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Resource {
    /// Civitai model identity.
    pub model_id: u64,
    /// Selected version identity.
    pub version_id: u64,
    /// Exact file identity.
    pub file_id: u64,
    /// Resource display name.
    pub name: String,
    /// Version display name.
    pub version: String,
    /// Declared architecture.
    pub base_model: String,
    /// Original filename, for display only.
    pub file_name: String,
    /// Advertised file size in KiB.
    pub size_kb: f64,
    /// Expected integrity digest.
    pub sha256: String,
    /// Official download endpoint.
    pub url: String,
    /// Requested resource type.
    pub kind: Kind,
    /// Suggested prompt trigger words.
    pub trained_words: Vec<String>,
}
/// Search result page; pass its opaque cursor to fetch the next page.
pub struct Page {
    /// Available safetensors files with SHA256 hashes.
    pub resources: Vec<Resource>,
    /// Opaque next-page cursor.
    pub cursor: Option<String>,
}
#[derive(Deserialize)]
struct Response {
    items: Vec<Model>,
    #[serde(default)]
    metadata: Metadata,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    next_cursor: Option<serde_json::Value>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Model {
    id: u64,
    name: String,
    #[serde(rename = "modelVersions")]
    versions: Vec<Version>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Version {
    id: u64,
    name: String,
    base_model: String,
    files: Vec<ModelFile>,
    #[serde(default)]
    trained_words: Vec<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelFile {
    id: u64,
    name: String,
    #[serde(rename = "sizeKB")]
    size_kb: f64,
    download_url: String,
    #[serde(default)]
    hashes: HashMap<String, String>,
    #[serde(default)]
    metadata: HashMap<String, serde_json::Value>,
}

fn client() -> Result<reqwest::blocking::Client, EditError> {
    reqwest::blocking::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .user_agent("MuFoEd-ImageStudio/0.1")
        .build()
        .map_err(|_| EditError::Invalid("Could not initialize HTTPS client".into()))
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
fn network_error(error: &reqwest::Error) -> EditError {
    EditError::Invalid(if let Some(status) = error.status() {
        format!("Civitai HTTP {status}; check account access/token or retry later")
    } else {
        "Civitai connection failed or timed out".into()
    })
}
/// Query the official catalog. No network activity occurs until this is called explicitly.
pub fn search(
    query: &str,
    kind: Kind,
    base: &str,
    cursor: Option<&str>,
    token: &str,
) -> Result<Page, EditError> {
    let mut request = client()?
        .get("https://civitai.com/api/v1/models")
        .timeout(Duration::from_secs(30))
        .query(&[
            ("limit", "20"),
            ("query", query),
            (
                "types",
                if kind == Kind::Lora {
                    "LORA"
                } else {
                    "Checkpoint"
                },
            ),
            ("sort", "Most Downloaded"),
            ("nsfw", "false"),
        ]);
    if !base.is_empty() {
        request = request.query(&[("baseModels", base)]);
    }
    if let Some(cursor) = cursor {
        request = request.query(&[("cursor", cursor)]);
    }
    let response = authorize(request, token)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|e| network_error(&e))?;
    parse_page(response.take(8 * 1024 * 1024), kind)
}
fn parse_page(reader: impl Read, kind: Kind) -> Result<Page, EditError> {
    let data: Response = serde_json::from_reader(reader)?;
    let mut resources = Vec::new();
    for model in data.items {
        for version in model.versions {
            for file in version.files {
                if file
                    .metadata
                    .get("format")
                    .and_then(serde_json::Value::as_str)
                    != Some("SafeTensor")
                    || !file.name.to_lowercase().ends_with(".safetensors")
                {
                    continue;
                }
                let Some(hash) = file.hashes.get("SHA256") else {
                    continue;
                };
                if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                    continue;
                }
                resources.push(Resource {
                    model_id: model.id,
                    version_id: version.id,
                    file_id: file.id,
                    name: model.name.clone(),
                    version: version.name.clone(),
                    base_model: version.base_model.clone(),
                    file_name: file.name,
                    size_kb: file.size_kb,
                    sha256: hash.clone(),
                    url: file.download_url,
                    kind,
                    trained_words: version.trained_words.clone(),
                });
            }
        }
    }
    Ok(Page {
        resources,
        cursor: data.metadata.next_cursor.and_then(|v| match v {
            serde_json::Value::String(s) => Some(s),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        }),
    })
}
/// Stream to a temporary file, verify SHA256 and tensor role, then publish without overwrite.
/// Progress is bytes received and the advertised total (if available).
pub fn download(
    resource: &Resource,
    root: &Path,
    token: &str,
    cancelled: &Arc<AtomicBool>,
    progress: impl Fn(u64, Option<u64>),
) -> Result<Asset, EditError> {
    let url = reqwest::Url::parse(&resource.url)
        .map_err(|_| EditError::Invalid("Invalid download URL".into()))?;
    if url.scheme() != "https"
        || url.host_str() != Some("civitai.com")
        || !url.path().starts_with("/api/download/models/")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(EditError::Invalid(
            "Expected an official Civitai download endpoint".into(),
        ));
    }
    let family = Family::parse(&resource.base_model);
    if family == Family::Unknown {
        return Err(EditError::Invalid(
            "This architecture is not supported by the current generation adapter".into(),
        ));
    }
    if resource.sha256.len() != 64 || !resource.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(EditError::Invalid("Missing valid SHA256".into()));
    }
    std::fs::create_dir_all(root)?;
    let name = format!(
        "civitai-{}-{}-{}.safetensors",
        resource.model_id, resource.version_id, resource.file_id
    );
    let destination = root.join(name);
    if destination.exists() {
        return Err(EditError::Invalid(
            "This file is already downloaded; refresh the local library".into(),
        ));
    }
    let response = authorize(client()?.get(url), token)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|e| network_error(&e))?;
    let total = response.content_length();
    store_verified(
        response,
        resource,
        root,
        &destination,
        total,
        cancelled,
        progress,
    )
}
fn store_verified(
    mut response: impl Read,
    resource: &Resource,
    root: &Path,
    destination: &Path,
    total: Option<u64>,
    cancelled: &Arc<AtomicBool>,
    progress: impl Fn(u64, Option<u64>),
) -> Result<Asset, EditError> {
    let family = Family::parse(&resource.base_model);
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    let mut hash = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = vec![0; 131_072];
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(EditError::Invalid("Download cancelled".into()));
        }
        let n = response.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        if bytes > 32 * 1024 * 1024 * 1024 {
            return Err(EditError::Invalid("Download exceeds 32 GiB limit".into()));
        }
        file.write_all(&buffer[..n])?;
        hash.update(&buffer[..n]);
        progress(bytes, total);
    }
    if !format!("{:x}", hash.finalize()).eq_ignore_ascii_case(&resource.sha256) {
        return Err(EditError::Invalid(
            "SHA256 mismatch; downloaded file was discarded".into(),
        ));
    }
    file.flush()?;
    let mut asset = assets::inspect(file.path())?;
    if asset.kind != resource.kind || (asset.family != Family::Unknown && asset.family != family) {
        return Err(EditError::Invalid(
            "Downloaded tensors do not match the selected resource".into(),
        ));
    }
    if cancelled.load(Ordering::Acquire) {
        return Err(EditError::Invalid("Download cancelled".into()));
    }
    file.persist_noclobber(destination)
        .map_err(|e| EditError::Io(e.error))?;
    // Receipt contains public provenance only. Never persist authentication data or signed URLs.
    let mut receipt = resource.clone();
    receipt.url.clear();
    std::fs::write(
        destination.with_extension("civitai.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    destination.clone_into(&mut asset.path);
    asset.name = format!("{} · {}", resource.name, resource.version);
    asset.family = family;
    Ok(asset)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Resource, Vec<u8>) {
        let header=br#"{"lora_unet_a.lora_down.weight":{},"__metadata__":{"ss_base_model_version":"sdxl_base_v1-0"}}"#;
        let bytes = [(header.len() as u64).to_le_bytes().as_slice(), header].concat();
        let resource = Resource {
            model_id: 1,
            version_id: 2,
            file_id: 3,
            name: "Fixture".into(),
            version: "v1".into(),
            base_model: "SDXL 1.0".into(),
            file_name: "../../unsafe.safetensors".into(),
            size_kb: 1.0,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            url: "https://civitai.com/api/download/models/2".into(),
            kind: Kind::Lora,
            trained_words: vec![],
        };
        (resource, bytes)
    }
    #[test]
    fn verified_publish_is_atomic_and_never_overwrites() {
        let (resource, bytes) = fixture();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("safe.safetensors");
        let cancel = Arc::new(AtomicBool::new(false));
        let asset = store_verified(
            bytes.as_slice(),
            &resource,
            dir.path(),
            &path,
            None,
            &cancel,
            |_, _| {},
        )
        .unwrap();
        assert_eq!(asset.path, path);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let receipt: Resource =
            serde_json::from_slice(&std::fs::read(path.with_extension("civitai.json")).unwrap())
                .unwrap();
        assert!(receipt.url.is_empty());
        assert_eq!(receipt.sha256, resource.sha256);
        assert!(store_verified(
            bytes.as_slice(),
            &resource,
            dir.path(),
            &path,
            None,
            &cancel,
            |_, _| {}
        )
        .is_err());
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    #[test]
    fn corrupt_and_cancelled_downloads_leave_no_weight_files() {
        let (resource, mut bytes) = fixture();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("safe.safetensors");
        let cancel = Arc::new(AtomicBool::new(false));
        bytes.push(0);
        assert!(store_verified(
            bytes.as_slice(),
            &resource,
            dir.path(),
            &path,
            None,
            &cancel,
            |_, _| {}
        )
        .is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        cancel.store(true, Ordering::Release);
        assert!(store_verified(
            bytes.as_slice(),
            &resource,
            dir.path(),
            &path,
            None,
            &cancel,
            |_, _| {}
        )
        .is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[test]
    fn catalog_keeps_versions_and_drops_pickle_and_missing_hash_files() {
        let (resource, _) = fixture();
        let file = serde_json::json!({"id":3,"name":"weights.safetensors","sizeKB":1,"downloadUrl":resource.url,"hashes":{"SHA256":resource.sha256},"metadata":{"format":"SafeTensor"}});
        let mut pickle = file.clone();
        pickle["metadata"]["format"] = "PickleTensor".into();
        let mut nohash = file.clone();
        nohash["hashes"] = serde_json::json!({});
        let data = serde_json::json!({"items":[{"id":1,"name":"Fixture","modelVersions":[{"id":2,"name":"v1","baseModel":"SDXL 1.0","files":[file.clone(),pickle,nohash]},{"id":4,"name":"v2","baseModel":"SDXL 1.0","files":[file]}]}],"metadata":{"nextCursor":"opaque|cursor"}});
        let page = parse_page(serde_json::to_vec(&data).unwrap().as_slice(), Kind::Lora).unwrap();
        assert_eq!(page.resources.len(), 2);
        assert_eq!(page.resources[1].version_id, 4);
        assert_eq!(page.cursor.as_deref(), Some("opaque|cursor"));
    }
}
