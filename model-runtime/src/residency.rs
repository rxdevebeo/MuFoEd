//! Host-owned residency and preflight. Backend callbacks never run under registry locks.
use crate::{
    telemetry::{Observation, Telemetry},
    Error, Lease, Memory, Phase, Runtime,
};
use std::{
    any::Any,
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

/// Memory policy, applied to fresh observations before backend allocation.
#[derive(Clone, Debug)]
pub struct MemoryPolicy {
    /// Host reserve in bytes (at least 5% of physical RAM is also reserved).
    pub reserve_ram: u64,
    /// GPU reserve in bytes (at least 10% of physical VRAM is also reserved).
    pub reserve_vram: u64,
    /// Maximum age accepted for a measurement.
    pub max_age: Duration,
    /// Reject unknown estimates and telemetry instead of attempting allocation.
    pub strict: bool,
    /// Maximum cached CPU bytes; least recently used idle entries are evicted.
    pub cache_ram: u64,
}
impl Default for MemoryPolicy {
    fn default() -> Self {
        Self {
            reserve_ram: 2 << 30,
            reserve_vram: 1 << 30,
            max_age: Duration::from_secs(5),
            strict: true,
            cache_ram: 12 << 30,
        }
    }
}
/// A backend resource. Destruction must be acknowledged before returning success.
pub trait Resource: Any + Send + Sync {
    /// Current resident bytes (not a predicted inference peak).
    fn memory(&self) -> Memory;
    /// Move GPU state to CPU when supported. `false` means unload is required.
    fn offload(&self) -> Result<bool, String>;
    /// Destroy all owned model state; return only after confirmation.
    fn unload(&self) -> Result<(), String>;
}
struct Entry {
    value: Arc<dyn Any + Send + Sync>,
    control: Arc<dyn Resource>,
    pins: Arc<AtomicUsize>,
    last: Instant,
    hits: u64,
}
/// One model cache row for UI/diagnostics.
#[derive(Clone, Debug)]
pub struct ResidentInfo {
    /// Complete compatibility identity (hashed by adapters).
    pub key: String,
    /// Current resource footprint.
    pub memory: Memory,
    /// Active use count; active entries cannot be evicted.
    pub pins: usize,
    /// Warm acquisitions since initial load.
    pub hits: u64,
}
/// Result of the most recent measured admission.
#[derive(Clone, Debug)]
pub struct Admission {
    /// Fresh driver/OS observation.
    pub observation: Observation,
    /// Selected physical device UUID, or CPU.
    pub device: String,
    /// Requested peak bytes.
    pub estimate: Memory,
    /// Error returned before allocation, if any.
    pub error: Option<String>,
}
pub(crate) struct Manager {
    pub policy: MemoryPolicy,
    pub telemetry: Arc<dyn Telemetry>,
    entries: Mutex<HashMap<String, Entry>>,
    last: Mutex<Option<Admission>>,
    quarantine: Mutex<Option<String>>,
}
impl Manager {
    pub(crate) fn new(policy: MemoryPolicy, telemetry: Arc<dyn Telemetry>) -> Self {
        Self {
            policy,
            telemetry,
            entries: Mutex::new(HashMap::new()),
            last: Mutex::new(None),
            quarantine: Mutex::new(None),
        }
    }
}
/// Pinned resource, owned by an inference stage. Eviction cannot destroy it.
pub struct Pinned<'lease, T> {
    _lease: std::marker::PhantomData<&'lease Lease>,
    value: Arc<T>,
    pins: Arc<AtomicUsize>,
}
impl<T> std::ops::Deref for Pinned<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}
impl<T> Drop for Pinned<'_, T> {
    fn drop(&mut self) {
        self.pins.fetch_sub(1, Ordering::AcqRel);
    }
}
impl Runtime {
    /// Whether this runtime measures physical memory and owns a resident registry.
    pub fn is_managed(&self) -> bool {
        self.0.manager.is_some()
    }
    /// A failed owned release blocks new allocation until cleanup is confirmed.
    pub fn blocked_reason(&self) -> Option<String> {
        self.0.manager.as_ref().and_then(|m| {
            m.quarantine
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        })
    }
    /// Latest preflight observation; reading never probes the driver.
    pub fn admission(&self) -> Option<Admission> {
        self.0.manager.as_ref().and_then(|m| {
            m.last
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        })
    }
    /// Inspect the model cache without probing the driver or invoking unload.
    pub fn residents(&self) -> Vec<ResidentInfo> {
        let Some(m) = &self.0.manager else {
            return Vec::new();
        };
        // Clone controls under lock, ask backend footprints outside it.
        let entries: Vec<_> = m
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(key, e)| (key.clone(), e.control.clone(), e.pins.clone(), e.hits))
            .collect();
        entries
            .into_iter()
            .map(|(key, control, pins, hits)| ResidentInfo {
                key,
                memory: control.memory(),
                pins: pins.load(Ordering::Acquire),
                hits,
            })
            .collect()
    }
}
impl Lease {
    /// Reuse an exact compatible resource, or publish it only after successful loading.
    pub fn cached<T: Resource>(
        &self,
        key: &str,
        load: impl FnOnce() -> Result<T, String>,
    ) -> Result<Pinned<'_, T>, String> {
        let Some(m) = &self.runtime.0.manager else {
            let value = Arc::new(load()?);
            return Ok(Pinned {
                _lease: std::marker::PhantomData,
                value,
                pins: Arc::new(AtomicUsize::new(1)),
            });
        };
        {
            let mut entries = m
                .entries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(e) = entries.get_mut(key) {
                let value = e
                    .value
                    .clone()
                    .downcast::<T>()
                    .map_err(|_| "Cache resource type mismatch")?;
                e.last = Instant::now();
                e.hits += 1;
                e.pins.fetch_add(1, Ordering::AcqRel);
                return Ok(Pinned {
                    _lease: std::marker::PhantomData,
                    value,
                    pins: e.pins.clone(),
                });
            }
        }
        self.set_phase(Phase::Loading);
        let value = Arc::new(load()?);
        let pins = Arc::new(AtomicUsize::new(1));
        m.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                key.into(),
                Entry {
                    value: value.clone(),
                    control: value.clone(),
                    pins: pins.clone(),
                    last: Instant::now(),
                    hits: 0,
                },
            );
        Ok(Pinned {
            _lease: std::marker::PhantomData,
            value,
            pins,
        })
    }
    #[cfg(feature = "ollama")]
    pub(crate) fn remove_confirmed(&self, key: &str) {
        if let Some(m) = &self.runtime.0.manager {
            m.entries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(key);
        }
    }
    /// Test residency without acquiring/adopting a foreign resource.
    pub fn contains(&self, key: &str) -> bool {
        self.runtime.0.manager.as_ref().is_some_and(|m| {
            m.entries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains_key(key)
        })
    }
    /// Unload one owned idle resource, preserving unrelated compatible cache entries.
    pub fn release_resource(&self, key: &str) -> Result<(), String> {
        let Some(m) = &self.runtime.0.manager else {
            return Ok(());
        };
        let resource = m
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(key)
            .map(|e| (e.control.clone(), e.pins.clone()));
        let Some((control, pins)) = resource else {
            return Ok(());
        };
        if pins.load(Ordering::Acquire) > 0 {
            return Err("resource is active".into());
        }
        self.set_phase(Phase::Releasing);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| control.unload()))
            .unwrap_or_else(|_| Err("backend panicked during unload".into()));
        if let Err(error) = result {
            *m.quarantine
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.clone());
            self.set_phase(Phase::Unknown);
            return Err(error);
        }
        m.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(key);
        Ok(())
    }
    /// Release idle resources oldest first. Never unload pinned/active entries.
    pub fn release_idle(&self) -> Result<(), String> {
        self.evict(true, None)
    }
    fn evict(&self, all: bool, resource: Option<&str>) -> Result<(), String> {
        let Some(m) = &self.runtime.0.manager else {
            return Ok(());
        };
        let mut candidates: Vec<_> = m
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|(_, e)| e.pins.load(Ordering::Acquire) == 0)
            .map(|(k, e)| (k.clone(), e.last, e.control.clone()))
            .collect();
        candidates.retain(|(_, _, control)| match resource {
            Some("VRAM") => control.memory().vram != Some(0),
            Some("RAM") => control.memory().ram != Some(0),
            _ => true,
        });
        candidates.sort_by_key(|(_, at, _)| *at);
        for (key, _, control) in candidates {
            self.set_phase(Phase::Offloading);
            let observation = m.telemetry.observe();
            let bytes = control.memory();
            let can_offload = observation.ram_available.is_some_and(|available| {
                available.saturating_sub(m.policy.reserve_ram) > bytes.vram.unwrap_or(0)
            });
            if !all
                && resource != Some("RAM")
                && bytes.vram.unwrap_or(0) > 0
                && can_offload
                && matches!(
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| control.offload())),
                    Ok(Ok(true))
                )
            {
                return Ok(());
            }
            self.set_phase(Phase::Releasing);
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| control.unload()))
                    .unwrap_or_else(|_| Err("backend panicked during unload".into()));
            if let Err(error) = result {
                self.set_phase(Phase::Unknown);
                *m.quarantine
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.clone());
                return Err(error);
            }

            m.entries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&key);
            if !all {
                break;
            }
        }
        if all {
            *m.quarantine
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }
        Ok(())
    }
    /// Resolve a stable device identity without creating a tensor device/context.
    pub fn device_identity(&self, device: &str) -> Result<String, Error> {
        if device == "cpu" {
            return Ok("cpu".into());
        }
        let Some(m) = &self.runtime.0.manager else {
            return Ok(device.into());
        };
        m.telemetry
            .observe()
            .gpu(device)
            .map(|g| g.uuid.clone())
            .ok_or_else(|| {
                Error::Telemetry(
                    "GPU identity unavailable or ambiguous; bind a CUDA GPU UUID".into(),
                )
            })
    }
    /// Fresh preflight after exclusive admission, on a worker. Rechecks after each eviction.
    /// A failed preflight never starts backend allocation and leaves no ready cache entry.
    #[allow(clippy::too_many_lines)] // Remeasure/evict/admit loop keeps every failure before allocation.
    pub fn prepare(&self, estimate: Memory, device: &str) -> Result<String, Error> {
        self.prepare_reusing(estimate, device, &[])
    }
    #[allow(clippy::too_many_lines)] // Remeasure/evict/admit loop keeps every failure before allocation.
    /// Check additional allocations using exact compatible owned cache keys.
    /// Reuse is revalidated after admission and after each eviction.
    pub fn prepare_reusing(
        &self,
        estimate: Memory,
        device: &str,
        reuse: &[String],
    ) -> Result<String, Error> {
        let mut fixed = Ok(());
        for (resource, requested, available) in [
            ("RAM", estimate.ram, self.runtime.0.policy.ram),
            ("VRAM", estimate.vram, self.runtime.0.policy.vram),
        ] {
            if let Some(available) = available {
                match requested {
                    None => {
                        fixed = Err(Error::UnknownEstimate(resource));
                        break;
                    }
                    Some(requested) if requested > available => {
                        fixed = Err(Error::OverBudget {
                            resource,
                            requested,
                            available,
                        });
                        break;
                    }
                    _ => {}
                }
            }
        }
        let Some(m) = &self.runtime.0.manager else {
            fixed?;
            return Ok(device.into());
        };
        if let Err(error) = fixed {
            *m.last
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Admission {
                observation: m.telemetry.observe(),
                device: device.into(),
                estimate,
                error: Some(error.to_string()),
            });
            return Err(error);
        }
        if m.quarantine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
        {
            self.release_idle().map_err(Error::Backend)?;
        }
        self.set_phase(Phase::Checking);
        let mut rounds = 0;
        loop {
            rounds += 1;
            if rounds > 128 {
                return Err(Error::Backend(
                    "memory availability did not stabilize after eviction".into(),
                ));
            }
            let observation = m.telemetry.observe();
            let selected = if device == "cpu" || estimate.vram == Some(0) {
                None
            } else {
                observation.gpu(device)
            };
            let identity = if estimate.vram == Some(0) || device == "cpu" {
                "cpu".to_owned()
            } else {
                selected.map_or_else(|| device.to_owned(), |g| g.uuid.clone())
            };
            // Recompute after every eviction; a queued cache hit may have disappeared.
            let mut additional = estimate;
            for entry in self
                .runtime
                .residents()
                .iter()
                .filter(|entry| reuse.contains(&entry.key))
            {
                additional.ram = additional
                    .ram
                    .map(|peak| peak.saturating_sub(entry.memory.ram.unwrap_or(0)));
                additional.vram = additional
                    .vram
                    .map(|peak| peak.saturating_sub(entry.memory.vram.unwrap_or(0)));
            }
            let outcome = (|| {
                if observation.at.elapsed() > m.policy.max_age {
                    return Err(Error::Telemetry("stale observation".into()));
                }
                for (resource, requested, available) in [
                    (
                        "RAM",
                        additional.ram,
                        observation.ram_available.map(|free| {
                            free.saturating_sub(
                                m.policy
                                    .reserve_ram
                                    .max(observation.ram_total.unwrap_or(0) / 20),
                            )
                        }),
                    ),
                    (
                        "VRAM",
                        additional.vram,
                        if estimate.vram == Some(0) {
                            Some(0)
                        } else {
                            selected.map(|g| {
                                g.free
                                    .saturating_sub(m.policy.reserve_vram.max(g.total / 10))
                            })
                        },
                    ),
                ] {
                    let Some(requested) = requested else {
                        if m.policy.strict {
                            return Err(Error::UnknownEstimate(resource));
                        }
                        continue;
                    };
                    let Some(available) = available else {
                        if m.policy.strict {
                            return Err(Error::Telemetry(format!(
                                "{resource} unavailable for {identity}"
                            )));
                        }
                        continue;
                    };
                    if requested > available {
                        return Err(Error::OverBudget {
                            resource,
                            requested,
                            available,
                        });
                    }
                }
                Ok(())
            })();
            *m.last
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Admission {
                observation,
                device: identity.clone(),
                estimate: additional,
                error: outcome.as_ref().err().map(ToString::to_string),
            });
            if matches!(outcome, Err(Error::OverBudget { .. })) {
                let before = self.runtime.residents().len();
                self.evict(
                    false,
                    outcome.as_ref().err().and_then(|e| {
                        if let Error::OverBudget { resource, .. } = e {
                            Some(*resource)
                        } else {
                            None
                        }
                    }),
                )
                .map_err(Error::Backend)?;
                if self.runtime.residents().len() < before {
                    continue;
                }
                // Offload can keep an entry while releasing GPU memory: remeasure once.
                let refreshed = m.telemetry.observe();
                let old = self
                    .runtime
                    .admission()
                    .expect("recorded observation")
                    .observation;
                if refreshed.gpus.iter().map(|g| g.free).sum::<u64>()
                    > old.gpus.iter().map(|g| g.free).sum()
                {
                    continue;
                }
            }
            outcome?;
            // Bound idle CPU residency without touching resources used by this stage.
            let before_trim = self.runtime.residents().len();
            while self
                .runtime
                .residents()
                .iter()
                .map(|e| e.memory.ram.unwrap_or(0))
                .sum::<u64>()
                > m.policy.cache_ram
            {
                let before = self.runtime.residents().len();
                self.evict(false, Some("RAM")).map_err(Error::Backend)?;
                if self.runtime.residents().len() == before {
                    break;
                }
            }
            if self.runtime.residents().len() < before_trim {
                continue;
            }
            {
                let mut state = self
                    .runtime
                    .0
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some((id, task)) = &mut state.active {
                    if *id == self.id {
                        task.memory = estimate;
                        task.device.clone_from(&identity);
                        task.reuse = reuse.to_vec();
                    }
                }
            }
            self.set_phase(Phase::Running);
            return Ok(identity);
        }
    }
    /// Enforce the idle CPU-cache budget at a transaction boundary.
    pub fn trim_cache(&self) -> Result<(), String> {
        let Some(m) = &self.runtime.0.manager else {
            return Ok(());
        };
        while self
            .runtime
            .residents()
            .iter()
            .map(|e| e.memory.ram.unwrap_or(0))
            .sum::<u64>()
            > m.policy.cache_ram
        {
            let before = self.runtime.residents().len();
            self.evict(false, Some("RAM"))?;
            if self.runtime.residents().len() == before {
                break;
            }
        }
        Ok(())
    }
    /// At most one retry of a transactional operation after confirmed cleanup.
    /// The closure must reproduce the exact request/seed and discard partial outputs.
    pub fn retry<T>(
        &self,
        estimate: Memory,
        device: &str,
        cancel: &std::sync::atomic::AtomicBool,
        mut operation: impl FnMut(u8) -> Result<T, String>,
    ) -> Result<T, String> {
        for attempt in 0..=1 {
            if cancel.load(Ordering::Acquire) {
                return Err("cancelled".into());
            }
            self.prepare(estimate, device).map_err(|e| e.to_string())?;
            match operation(attempt) {
                Ok(value) => {
                    self.trim_cache()?;
                    self.set_phase(Phase::Ready);
                    return Ok(value);
                }
                Err(error) if attempt == 0 && is_oom(&error) => {
                    self.release_idle()?;
                    self.set_phase(Phase::Retrying);
                }
                Err(error) => {
                    self.set_phase(Phase::Failed);
                    self.release_idle()
                        .map_err(|cleanup| format!("{error}; cleanup unconfirmed: {cleanup}"))?;
                    return Err(error);
                }
            }
        }
        unreachable!()
    }
}
/// Narrow backend OOM recognition; generic I/O or model errors must not trigger retries.
pub fn is_oom(error: &str) -> bool {
    let text = error.to_ascii_lowercase();
    [
        "cuda_error_out_of_memory",
        "cuda out of memory",
        "cuda error: out of memory",
        "failed to allocate memory",
        "out of memory",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

#[cfg(test)]
#[path = "residency_tests.rs"]
mod tests;
