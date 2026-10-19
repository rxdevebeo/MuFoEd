//! Owned Ollama operations with confirmed unloading and recovery after transport failure.
//! Loopback endpoints share the host runtime; remote endpoints have separate domains.
//! A model already resident outside this adapter is never adopted or unloaded implicitly.
use crate::{Lease, Phase, Policy, Runtime, Task};
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
}
impl Client {
    /// Resolve the resource domain. Local requests share `local`; remote requests
    /// use one independent process-wide domain for their normalized endpoint.
    pub fn new(endpoint: &str, model: &str, local: &Runtime) -> Result<Self, String> {
        let (endpoint, loopback) = endpoint_domain(endpoint)?;
        if model.trim().is_empty() {
            return Err("No Ollama model selected".into());
        }
        let runtime = if loopback {
            local.clone()
        } else {
            remote_runtime(&endpoint)
        };
        Ok(Self {
            endpoint,
            model: model.into(),
            runtime,
            transport: Arc::new(Http),
        })
    }
    /// Override bounded metadata/unload transport, e.g. for offline contract tests.
    #[must_use]
    pub fn with_transport(mut self, transport: Arc<dyn Transport>) -> Self {
        self.transport = transport;
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
        operation: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let task = Task::new(owner, format!("ollama:{}", self.model));
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
        if !self.absent(digest.as_deref())? {
            return Err("Selected Ollama model is already resident outside this manager; unload it in Ollama before requesting managed inference".into());
        }
        if cancel.load(Ordering::Acquire) {
            return Err("cancelled".into());
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
        let result = operation();
        let release = session.finish();
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
    fn finish(mut self) -> Result<(), String> {
        self.phase(Phase::Releasing);
        self.client.release(self.digest.as_deref())?;
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
