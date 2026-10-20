//! Owned Ollama operations with confirmed unloading and recovery after transport failure.
//! Loopback endpoints share the host runtime; remote endpoints have separate domains.
//! A model already resident outside this adapter is never adopted or unloaded implicitly.
use crate::{residency::Resource, Lease, Memory, Phase, Policy, Runtime, Task};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};

/// Backend request configuration. Every operation is synchronous on a worker thread.
pub trait Transport: Send + Sync {
    /// A bounded JSON metadata or unload request. `None` is GET, `Some` is POST.
    fn request(&self, endpoint: &str, path: &str, body: Option<&Value>) -> Result<Value, String>;
}

struct Http;
impl Transport for Http {
    fn request(&self, endpoint: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(3)))
            .timeout_global(Some(Duration::from_secs(10)))
            .build()
            .into();
        let url = format!("{endpoint}{path}");
        let mut response = match body {
            Some(body) => agent
                .post(&url)
                .header("content-type", "application/json")
                .send(body.to_string()),
            None => agent.get(&url).call(),
        }
        .map_err(|e| e.to_string())?;
        let text = response
            .body_mut()
            .with_config()
            .limit(8 * 1024 * 1024)
            .read_to_string()
            .map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }
}

/// An owned operation, visible while executing or waiting for confirmed release.
#[derive(Clone, Debug)]
pub struct OwnedModel {
    /// Process-unique operation identity; not a model-cache key.
    pub id: u64,
    /// Calling application subsystem.
    pub owner: String,
    /// Validated endpoint without credentials.
    pub endpoint: String,
    /// User-selected model, never another server model.
    pub model: String,
    /// Current lifecycle stage.
    pub phase: Phase,
}
fn registry() -> &'static Mutex<HashMap<u64, OwnedModel>> {
    static REGISTRY: OnceLock<Mutex<HashMap<u64, OwnedModel>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}
/// Inspect only models whose operations this adapter owns.
pub fn owned_models() -> Vec<OwnedModel> {
    registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .values()
        .cloned()
        .collect()
}

/// Coordinated Ollama endpoint. Construct on a worker, then call [`Client::run`].
#[derive(Clone)]
pub struct Client {
    endpoint: String,
    model: String,
    runtime: Runtime,
    transport: Arc<dyn Transport>,
    context: Option<u32>,
}
impl Client {
    /// Resolve the resource domain. Local requests share `local`; remote requests
    /// use one independent process-wide domain for their normalized endpoint.
    pub fn new(endpoint: &str, model: &str, local: &Runtime) -> Result<Self, String> {
        if local.is_shutting_down() {
            return Err(crate::Error::ShuttingDown.to_string());
        }
        let (endpoint, loopback) = endpoint_domain(endpoint)?;
        if model.trim().is_empty() {
            return Err("No Ollama model selected".into());
        }
        let runtime = if loopback {
            local.clone()
        } else {
            remote_runtime(&endpoint)
        };
        local.link_domain(&runtime);
        Ok(Self {
            endpoint,
            model: model.into(),
            runtime,
            transport: Arc::new(Http),
            context: None,
        })
    }
    /// Override bounded metadata/unload transport, e.g. for offline contract tests.
    #[must_use]
    pub fn with_transport(mut self, transport: Arc<dyn Transport>) -> Self {
        self.transport = transport;
        self
    }
    /// Context bound actually sent by the application, including vision prompt tokens.
    #[must_use]
    pub fn with_context(mut self, tokens: u32) -> Self {
        self.context = Some(tokens);
        self
    }
    /// The shared domain used by this endpoint.
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    /// Run an existing backend under admission, then unload only its selected model.
    /// `operation` must remain synchronous until its HTTP client returns; it must not
    /// submit a detached task. Errors, cancellation and panic retain cleanup ownership.
    pub fn run<T>(
        &self,
        owner: &str,
        cancel: &AtomicBool,
        mut operation: impl FnMut() -> Result<T, String>,
    ) -> Result<T, String> {
        for attempt in 0..=1 {
            let result = self.run_once(owner, cancel, &mut operation);
            if attempt == 0
                && self.runtime.is_managed()
                && !cancel.load(Ordering::Acquire)
                && result.as_ref().err().is_some_and(|e| {
                    crate::residency::is_oom(e) && !e.contains("release unconfirmed")
                })
            {
                self.runtime
                    .release_idle_async()
                    .recv_timeout(Duration::from_secs(900))
                    .map_err(|e| e.to_string())??;
                continue;
            }
            return result;
        }
        unreachable!()
    }
    #[allow(clippy::too_many_lines)] // Metadata, owned compute and confirmed release form one transaction.
    fn run_once<T>(
        &self,
        owner: &str,
        cancel: &AtomicBool,
        operation: impl FnMut() -> Result<T, String>,
    ) -> Result<T, String> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let mut task = Task::new(owner, format!("ollama:{}", self.model));
        if self.runtime.is_managed() {
            let tags = self.transport.request(&self.endpoint, "/api/tags", None)?;
            let entry = tags["models"]
                .as_array()
                .and_then(|models| {
                    models.iter().find(|entry| {
                        names_match(entry["name"].as_str().unwrap_or_default(), &self.model)
                    })
                })
                .ok_or("Selected Ollama model is not installed")?;
            let show = self.transport.request(
                &self.endpoint,
                "/api/show",
                Some(&json!({"model":self.model})),
            )?;
            task.memory = estimate(entry, &show, self.context, owner);
            let key = self.cache_key(entry["digest"].as_str().ok_or("Missing model digest")?);
            task.reuse.push(key);
        }
        let lease = self
            .runtime
            .enqueue(task)
            .map_err(|e| e.to_string())?
            .wait(cancel, Duration::from_secs(900))
            .map_err(|e| e.to_string())?;
        if cancel.load(Ordering::Acquire) {
            return Err("cancelled".into());
        }
        // Metadata calls cannot load a model. Refuse adoption of pre-existing residency,
        // including another alias with the same digest; never unload someone else's model.
        let tags = self.transport.request(&self.endpoint, "/api/tags", None)?;
        let models = tags["models"]
            .as_array()
            .ok_or("Invalid Ollama model catalog")?;
        let entry = models
            .iter()
            .find(|entry| names_match(entry["name"].as_str().unwrap_or_default(), &self.model))
            .ok_or("Selected Ollama model is not installed")?;
        let digest = Some(
            entry["digest"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or("Ollama catalog did not provide the selected model digest")?
                .to_owned(),
        );
        let key = self.cache_key(digest.as_deref().expect("validated digest"));
        let prefix = format!(
            "ollama:{}:{}:",
            self.endpoint,
            digest.as_deref().expect("digest")
        );
        for entry in self
            .runtime
            .residents()
            .iter()
            .filter(|entry| entry.key.starts_with(&prefix) && entry.key != key)
        {
            lease.release_resource(&entry.key)?;
        }
        let resident = self.transport.request(&self.endpoint, "/api/ps", None)?;
        if let Some(entry) = resident["models"].as_array().and_then(|models| {
            models
                .iter()
                .find(|entry| entry["digest"].as_str() == digest.as_deref())
        }) {
            if lease.contains(&key) {
                let cached_model = lease
                    .cached::<ResidentOllama>(&key, || Err("owned cache disappeared".into()))?;
                let expiry = cached_model
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .1
                    .clone();
                if expiry != entry["expires_at"] {
                    drop(cached_model);
                    lease.remove_confirmed(&key);
                    return Err(
                        "Ollama residency changed outside this manager; ownership discarded".into(),
                    );
                }
            }
        }
        if !self.absent(digest.as_deref())? && !lease.contains(&key) {
            return Err("Selected Ollama model is already resident outside this manager; unload it in Ollama before requesting managed inference".into());
        }
        if cancel.load(Ordering::Acquire) {
            return Err("cancelled".into());
        }
        if self.runtime.is_managed() {
            if self.absent(digest.as_deref())? {
                lease.remove_confirmed(&key);
            }
            let show = self.transport.request(
                &self.endpoint,
                "/api/show",
                Some(&json!({"model":self.model})),
            )?;
            lease
                .prepare_reusing(
                    estimate(entry, &show, self.context, owner),
                    "auto",
                    std::slice::from_ref(&key),
                )
                .map_err(|e| e.to_string())?;
        }
        let id = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| "Ollama operation identity exhausted")?;
        registry()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                id,
                OwnedModel {
                    id,
                    owner: owner.into(),
                    endpoint: self.endpoint.clone(),
                    model: self.model.clone(),
                    phase: Phase::Running,
                },
            );
        let session = Session {
            client: self.clone(),
            lease: Some(lease),
            id,
            digest,
        };
        let mut operation = operation;
        let result = operation();
        let release =
            if result.is_ok() && self.runtime.is_managed() && !cancel.load(Ordering::Acquire) {
                session.finish_cached(&key)
            } else {
                session.finish()
            };
        match (result, release) {
            (result, Err(error)) => {
                let backend = result
                    .err()
                    .map_or_else(String::new, |error| format!("Backend: {error}. "));
                Err(format!("{backend}Model release unconfirmed; admission retained by recovery worker: {error}"))
            }
            (result, Ok(())) => {
                if cancel.load(Ordering::Acquire) {
                    Err("cancelled".into())
                } else {
                    result
                }
            }
        }
    }
    fn cache_key(&self, digest: &str) -> String {
        format!(
            "ollama:{}:{digest}:ctx{:?}:f16-kv",
            self.endpoint, self.context
        )
    }
    fn absent(&self, digest: Option<&str>) -> Result<bool, String> {
        let value = self.transport.request(&self.endpoint, "/api/ps", None)?;
        let models = value["models"]
            .as_array()
            .ok_or("Invalid Ollama residency response")?;
        Ok(!models.iter().any(|entry| {
            names_match(entry["name"].as_str().unwrap_or_default(), &self.model)
                || names_match(entry["model"].as_str().unwrap_or_default(), &self.model)
                || digest.is_some_and(|digest| entry["digest"].as_str() == Some(digest))
        }))
    }
    fn release(&self, digest: Option<&str>) -> Result<(), String> {
        // Do not use absence alone after a timeout: a still-loading request might
        // not yet appear in /api/ps. Wait for an explicit unload acknowledgement first.
        let value = self.transport.request(
            &self.endpoint,
            "/api/generate",
            Some(&json!({
                "model": self.model, "stream": false, "keep_alive": 0,
            })),
        )?;
        if value["done"] != true || value["done_reason"] != "unload" {
            return Err("Ollama did not acknowledge model unload".into());
        }
        if !self.absent(digest)? {
            return Err("Ollama still reports the selected model resident".into());
        }
        Ok(())
    }
}

fn estimate(entry: &Value, show: &Value, context: Option<u32>, owner: &str) -> Memory {
    let calculate = || {
        let weights = entry["size"].as_u64()?;
        let info = show["model_info"].as_object()?;
        let prefix = info.keys().find_map(|k| k.strip_suffix(".block_count"))?;
        let number = |suffix: &str| {
            info.get(&format!("{prefix}.{suffix}"))
                .and_then(Value::as_u64)
        };
        let layers = number("block_count")?;
        let heads = number("attention.head_count")?;
        let kv_heads = number("attention.head_count_kv").unwrap_or(heads);
        let key_dim = number("attention.key_length")
            .or_else(|| number("embedding_length").map(|n| n / heads.max(1)))?;
        let value_dim = number("attention.value_length").unwrap_or(key_dim);
        let context = u64::from(context.unwrap_or_else(|| {
            u32::try_from(number("context_length").unwrap_or(0)).unwrap_or(u32::MAX)
        }));
        if context == 0 {
            return None;
        }
        let kv = layers
            .saturating_mul(kv_heads)
            .saturating_mul(key_dim + value_dim)
            .saturating_mul(2)
            .saturating_mul(context);
        let workspace = if owner == "vision" {
            3_u64 << 30
        } else {
            1_u64 << 30
        };
        let peak = weights.saturating_add(kv).saturating_add(workspace);
        Some(Memory {
            ram: Some(peak + (1 << 30)),
            vram: Some(peak),
        })
    };
    calculate().unwrap_or_default()
}
struct ResidentOllama {
    client: Client,
    digest: Option<String>,
    state: Mutex<(Memory, Value)>,
}
impl Resource for ResidentOllama {
    fn memory(&self) -> Memory {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .0
    }
    fn offload(&self) -> Result<bool, String> {
        Ok(false)
    }
    fn unload(&self) -> Result<(), String> {
        self.client.release(self.digest.as_deref())
    }
}

fn names_match(left: &str, right: &str) -> bool {
    let canonical = |name: &str| {
        if name
            .rsplit('/')
            .next()
            .is_some_and(|last| last.contains(':'))
        {
            name.to_owned()
        } else {
            format!("{name}:latest")
        }
    };
    !left.is_empty() && canonical(left) == canonical(right)
}

/// Validate the endpoint and identify loopback without resolving arbitrary DNS names.
/// Non-loopback hosts (including LAN addresses) are separate domains.
pub fn endpoint_domain(endpoint: &str) -> Result<(String, bool), String> {
    let url = url::Url::parse(endpoint.trim()).map_err(|e| e.to_string())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "Ollama endpoint must be HTTP(S), without URL credentials, query or fragment".into(),
        );
    }
    let loopback = match url.host() {
        Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    Ok((url.as_str().trim_end_matches('/').to_owned(), loopback))
}
fn remote_runtime(endpoint: &str) -> Runtime {
    static DOMAINS: OnceLock<Mutex<HashMap<String, Runtime>>> = OnceLock::new();
    DOMAINS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(endpoint.into())
        .or_insert_with(|| Runtime::new(Policy::default()))
        .clone()
}

struct Session {
    client: Client,
    lease: Option<Lease>,
    id: u64,
    digest: Option<String>,
}
impl Session {
    fn phase(&self, phase: Phase) {
        if let Some(lease) = &self.lease {
            lease.set_phase(phase);
        }
        if let Some(record) = registry()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(&self.id)
        {
            record.phase = phase;
        }
    }
    fn finish_cached(mut self, key: &str) -> Result<(), String> {
        self.phase(Phase::Releasing);
        // Successful synchronous compute is terminal; inspect actual server residency.
        let value = self
            .client
            .transport
            .request(&self.client.endpoint, "/api/ps", None)?;
        let models = value["models"]
            .as_array()
            .ok_or("Invalid Ollama residency response")?;
        if let Some(model) = models
            .iter()
            .find(|entry| entry["digest"].as_str() == self.digest.as_deref())
        {
            let vram = model["size_vram"].as_u64();
            let mut client = self.client.clone();
            // Release callback needs only endpoint/transport, not the owning runtime (no cycle).
            client.runtime = Runtime::new(Policy::default());
            let resident = self.lease.as_ref().expect("owned lease").cached(key, || {
                Ok(ResidentOllama {
                    client,
                    digest: self.digest.clone(),
                    state: Mutex::new((Memory { ram: None, vram }, model["expires_at"].clone())),
                })
            })?;
            *resident
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                (Memory { ram: None, vram }, model["expires_at"].clone());
            drop(resident);
            self.lease.as_ref().expect("owned lease").trim_cache()?;
        } else {
            self.client.release(self.digest.as_deref())?;
            self.lease
                .as_ref()
                .expect("owned lease")
                .remove_confirmed(key);
        }
        registry()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.id);
        if let Some(lease) = &self.lease {
            lease.set_phase(Phase::Ready);
        }
        self.lease = None;
        Ok(())
    }
    fn finish(mut self) -> Result<(), String> {
        self.phase(Phase::Releasing);
        self.client.release(self.digest.as_deref())?;
        if let Some(lease) = &self.lease {
            lease.remove_confirmed(
                &self
                    .client
                    .cache_key(self.digest.as_deref().unwrap_or_default()),
            );
        }
        registry()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.id);
        self.lease = None;
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if self.lease.is_none() {
            return;
        }
        self.phase(Phase::Unknown);
        let pending = Pending {
            client: self.client.clone(),
            lease: self.lease.take().expect("owned lease"),
            id: self.id,
            digest: self.digest.clone(),
        };
        // Registry ownership survives thread-spawn failure; never drop an uncertain lease.
        recovery()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(pending);
        start_recovery();
    }
}
struct Pending {
    client: Client,
    lease: Lease,
    id: u64,
    digest: Option<String>,
}
fn recovery() -> &'static Mutex<Vec<Pending>> {
    static PENDING: OnceLock<Mutex<Vec<Pending>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(Vec::new()))
}
fn start_recovery() {
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("mufoed-model-release".into())
        .spawn(|| loop {
            // Network/backend work is outside registry locks. Keep failed entries alive.
            let entries = std::mem::take(
                &mut *recovery()
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
            for entry in entries {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    entry.client.release(entry.digest.as_deref())
                }));
                if matches!(result, Ok(Ok(()))) {
                    registry()
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .remove(&entry.id);
                    drop(entry.lease);
                } else {
                    recovery()
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(entry);
                }
            }
            std::thread::sleep(Duration::from_secs(2));
        });
    if spawned.is_err() {
        RUNNING.store(false, Ordering::Release);
    }
}

#[cfg(test)]
#[path = "ollama_tests.rs"]
mod tests;
