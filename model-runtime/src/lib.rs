//! Shared model admission for `MuFoEd`, independent of UI, tensor libraries and model type.
//!
//! Clone one [`Runtime`] into text, vision and image adapters. Hold a [`Lease`] until
//! inference and resource destruction actually finish, even after UI cancellation.
//! Admission reservations are estimates, not measurements of an allocator or free VRAM.
pub mod fingerprint;
#[cfg(feature = "ollama")]
pub mod ollama;
pub mod residency;
pub mod telemetry;
use std::{
    collections::VecDeque,
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

/// Estimated peak bytes, including weights, temporary tensors and backend overhead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Memory {
    /// Host memory; `None` means the backend has no calibrated estimate yet.
    pub ram: Option<u64>,
    /// Device memory; CPU-only jobs explicitly request `Some(0)`.
    pub vram: Option<u64>,
}

/// One backend operation. Model identity is diagnostic, never a cache compatibility key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    /// Calling subsystem, for example `text`, `vision`, or `image`.
    pub owner: String,
    /// Backend and model label without credentials.
    pub model: String,
    /// Exact execution device, or `auto` while resolution is backend-owned.
    pub device: String,
    /// Peak reservation. Unknown estimates remain visible to the host.
    pub memory: Memory,
    /// Exact compatible cache entries; additional peak is computed only after admission.
    pub reuse: Vec<String>,
}
impl Task {
    /// Operation with an uncalibrated estimate. Strict budgets will reject it.
    pub fn new(owner: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            model: model.into(),
            device: "auto".into(),
            memory: Memory::default(),
            reuse: Vec::new(),
        }
    }
}

/// Admission limits, already reduced by the host's renderer/OS reserve.
#[derive(Clone, Debug)]
pub struct Policy {
    /// Maximum estimated peak host bytes; `None` leaves this limit unconfigured.
    pub ram: Option<u64>,
    /// Maximum estimated peak device bytes; `None` leaves this limit unconfigured.
    pub vram: Option<u64>,
    /// Maximum number of waiting operations.
    pub queue_capacity: usize,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            ram: None,
            vram: None,
            queue_capacity: 64,
        }
    }
}

/// Admission failure. No backend allocation has been started for this task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The waiting queue is full.
    QueueFull,
    /// The host is closing; no new model operation may allocate.
    ShuttingDown,
    /// A configured budget requires a backend estimate.
    UnknownEstimate(&'static str),
    /// Peak estimate exceeds the configured usable budget.
    OverBudget {
        /// Resource name.
        resource: &'static str,
        /// Requested bytes.
        requested: u64,
        /// Usable bytes.
        available: u64,
    },
    /// Cancelled while waiting; running work is cancelled by its adapter.
    Cancelled,
    /// Queue wait expired.
    Timeout,
    /// A ticket can start only once.
    AlreadyStarted,
    /// Physical memory could not be measured reliably.
    Telemetry(String),
    /// Confirmed backend cleanup failed.
    Backend(String),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Model runtime: {self:?}")
    }
}
impl std::error::Error for Error {}

/// A consistent view of the shared queue, suitable for a host status panel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Backend currently holding exclusive inference admission.
    pub active: Option<Task>,
    /// Waiting tasks in FIFO order.
    pub queued: Vec<Task>,
    /// Current lifecycle stage of the active operation.
    pub phase: Option<Phase>,
}

/// Backend lifecycle stage; uncertain release continues to hold admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Fresh memory admission check.
    Checking,
    /// Model weights are loading.
    Loading,
    /// Resident GPU components are moving to host memory.
    Offloading,
    /// Retrying the same transaction once after OOM cleanup.
    Retrying,
    /// Transaction completed.
    Ready,
    /// Transaction failed.
    Failed,
    /// Model loading or inference is in progress.
    Running,
    /// Waiting for confirmed model release.
    Releasing,
    /// Release could not be confirmed; the recovery worker retains admission.
    Unknown,
}

/// A bounded lifecycle receipt; available after an operation ends.
#[derive(Clone, Debug)]
pub struct Transition {
    /// Local monotonic observation time.
    pub at: Instant,
    /// Backend phase.
    pub phase: Phase,
    /// Human-readable stage, such as CLIP-L or VAE decode.
    pub detail: String,
}
#[derive(Default)]
struct State {
    shutting_down: bool,
    events: VecDeque<Transition>,
    next_id: u64,
    active: Option<(u64, Task)>,
    queued: VecDeque<(u64, Task)>,
    phase: Option<Phase>,
}
struct Inner {
    domains: Mutex<Vec<std::sync::Weak<Inner>>>,
    manager: Option<residency::Manager>,
    policy: Policy,
    state: Mutex<State>,
    changed: Condvar,
}

/// Shared, thread-safe model scheduler. Initial policy serializes all admitted inference
/// (including CPU work) to avoid competing model loads and host-memory peaks.
#[derive(Clone)]
pub struct Runtime(Arc<Inner>);
impl Runtime {
    /// Construct an independent host-owned runtime. Clones share the same queue.
    pub fn new(policy: Policy) -> Self {
        Self(Arc::new(Inner {
            policy,
            manager: None,
            domains: Mutex::new(Vec::new()),
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
        }))
    }
    /// Host-owned measured runtime with injectable telemetry and residency policy.
    pub fn managed(
        policy: Policy,
        memory: residency::MemoryPolicy,
        telemetry: Arc<dyn telemetry::Telemetry>,
    ) -> Self {
        Self(Arc::new(Inner {
            policy,
            manager: Some(residency::Manager::new(memory, telemetry)),
            domains: Mutex::new(Vec::new()),
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
        }))
    }
    /// Default process-wide instance shared by independently constructed editors/adapters.
    /// Separate OS processes require a broker; this is intentionally process-local.
    pub fn global() -> Self {
        static GLOBAL: OnceLock<Runtime> = OnceLock::new();
        GLOBAL
            .get_or_init(|| {
                Self::managed(
                    Policy::default(),
                    residency::MemoryPolicy::default(),
                    Arc::new(telemetry::SystemTelemetry),
                )
            })
            .clone()
    }
    /// Permanently close this host runtime, reject waiting/new producers, then
    /// confirm idle destruction after the current lease ends. Never interrupts compute.
    pub fn shutdown_async(&self) -> std::sync::mpsc::Receiver<Result<(), String>> {
        self.0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .shutting_down = true;
        self.0.changed.notify_all();
        let domains: Vec<_> = self
            .0
            .domains
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter_map(std::sync::Weak::upgrade)
            .map(Runtime)
            .collect();
        let mut releases = vec![self.release_idle_async()];
        releases.extend(domains.iter().map(Runtime::shutdown_async));
        let (sender, receiver) = std::sync::mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("model-host-shutdown".into())
            .spawn(move || {
                let result = (|| {
                    for release in releases {
                        release
                            .recv_timeout(Duration::from_secs(900))
                            .map_err(|e| e.to_string())??;
                    }
                    Ok(())
                })();
                let _ = sender.send(result);
            });
        receiver
    }
    /// Whether this host/domain permanently rejects new producers.
    pub fn is_shutting_down(&self) -> bool {
        self.0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .shutting_down
    }
    #[cfg(feature = "ollama")]
    pub(crate) fn link_domain(&self, domain: &Runtime) {
        if Arc::ptr_eq(&self.0, &domain.0) {
            return;
        }
        let mut domains = self
            .0
            .domains
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !domains
            .iter()
            .any(|weak| weak.ptr_eq(&Arc::downgrade(&domain.0)))
        {
            domains.push(Arc::downgrade(&domain.0));
        }
    }
    /// Queue explicit idle-cache release on a worker; active inference is never interrupted.
    pub fn release_idle_async(&self) -> std::sync::mpsc::Receiver<Result<(), String>> {
        let runtime = self.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("model-cache-release".into())
            .spawn(move || {
                let result = (|| {
                    let mut task = Task::new("host", "release idle models");
                    task.device = "cpu".into();
                    task.memory = Memory {
                        ram: Some(0),
                        vram: Some(0),
                    };
                    let lease = runtime
                        .enqueue_inner(task, true)
                        .map_err(|e| e.to_string())?
                        .wait(&AtomicBool::new(false), Duration::from_secs(900))
                        .map_err(|e| e.to_string())?;
                    lease.release_idle()?;
                    let _ = lease.prepare(
                        Memory {
                            ram: Some(0),
                            vram: Some(0),
                        },
                        "cpu",
                    );
                    Ok(())
                })();
                let _ = sender.send(result);
            });
        receiver
    }
    /// Recent model transitions; bounded to 64 entries per host runtime.
    pub fn events(&self) -> Vec<Transition> {
        self.0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .events
            .iter()
            .cloned()
            .collect()
    }
    /// Validate reservations and enqueue without loading a model or blocking the UI.
    pub fn enqueue(&self, task: Task) -> Result<Ticket, Error> {
        self.enqueue_inner(task, false)
    }
    fn enqueue_inner(&self, task: Task, maintenance: bool) -> Result<Ticket, Error> {
        for (resource, requested, available) in [
            ("RAM", task.memory.ram, self.0.policy.ram),
            ("VRAM", task.memory.vram, self.0.policy.vram),
        ] {
            if let Some(available) = available {
                let requested = requested.ok_or(Error::UnknownEstimate(resource))?;
                if requested > available {
                    return Err(Error::OverBudget {
                        resource,
                        requested,
                        available,
                    });
                }
            }
        }
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.shutting_down && !maintenance {
            return Err(Error::ShuttingDown);
        }
        if !maintenance && state.queued.len() >= self.0.policy.queue_capacity {
            return Err(Error::QueueFull);
        }
        // A process cannot realistically admit 2^64 jobs. Do not wrap into live IDs.
        let id = state.next_id.checked_add(1).ok_or(Error::QueueFull)?;
        state.next_id = id;
        state.queued.push_back((id, task));
        self.0.changed.notify_all();
        Ok(Ticket {
            runtime: self.clone(),
            id: Some(id),
            preparing: None,
            maintenance,
        })
    }
    /// Inspect active and queued work without calling a backend.
    pub fn snapshot(&self) -> Snapshot {
        let state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Snapshot {
            active: state.active.as_ref().map(|(_, task)| task.clone()),
            queued: state.queued.iter().map(|(_, task)| task.clone()).collect(),
            phase: state.phase,
        }
    }
}

/// A queued operation. Dropping it cancels only that queue entry.
pub struct Ticket {
    runtime: Runtime,
    id: Option<u64>,
    preparing: Option<std::sync::mpsc::Receiver<Result<Lease, Error>>>,
    maintenance: bool,
}
impl Ticket {
    /// Nonblocking admission. Only the head can start, and only when no lease exists.
    pub fn try_start(&mut self) -> Result<Option<Lease>, Error> {
        if let Some(receiver) = &self.preparing {
            return match receiver.try_recv() {
                Ok(result) => {
                    self.preparing = None;
                    result.map(Some)
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => Ok(None),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.preparing = None;
                    Err(Error::Backend("preflight worker stopped".into()))
                }
            };
        }
        let id = self.id.ok_or(Error::AlreadyStarted)?;
        let mut state = self
            .runtime
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.shutting_down && !self.maintenance {
            return Err(Error::ShuttingDown);
        }
        if state.active.is_some() || state.queued.front().map(|(id, _)| *id) != Some(id) {
            return Ok(None);
        }
        state.active = state.queued.pop_front();
        state.phase = Some(Phase::Running);
        self.id = None;
        let lease = Lease {
            runtime: self.runtime.clone(),
            id,
        };
        if self.runtime.is_managed() && !self.maintenance {
            let task = state.active.as_ref().expect("admitted").1.clone();
            drop(state);
            let (sender, receiver) = std::sync::mpsc::channel();
            std::thread::Builder::new()
                .name("model-preflight".into())
                .spawn(move || {
                    let result = match lease.prepare_reusing(task.memory, &task.device, &task.reuse)
                    {
                        Ok(_) => Ok(lease),
                        Err(error) => {
                            lease.set_phase(if lease.runtime.blocked_reason().is_some() {
                                Phase::Unknown
                            } else {
                                Phase::Failed
                            });
                            Err(error)
                        }
                    };
                    let _ = sender.send(result);
                })
                .map_err(|e| Error::Backend(e.to_string()))?;
            self.preparing = Some(receiver);
            Ok(None)
        } else {
            Ok(Some(lease))
        }
    }
    /// Wait on a backend worker thread, with cancellation and a bounded deadline.
    /// Never call this on an egui thread. Cancellation does not interrupt an active lease.
    pub fn wait(mut self, cancelled: &AtomicBool, timeout: Duration) -> Result<Lease, Error> {
        let began = Instant::now();
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err(Error::Cancelled);
            }
            if began.elapsed() >= timeout {
                return Err(Error::Timeout);
            }
            if let Some(lease) = self.try_start()? {
                return Ok(lease);
            }
            let state = self
                .runtime
                .0
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            // Recheck under the condition-variable lock: avoid missing a release notification.
            if self.id.is_some()
                && state.active.is_none()
                && state.queued.front().map(|(id, _)| *id) == self.id
            {
                continue;
            }
            let _wait = self.runtime.0.changed.wait_timeout(
                state,
                Duration::from_millis(50).min(timeout.saturating_sub(began.elapsed())),
            );
        }
    }
}
impl Drop for Ticket {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            let mut state = self
                .runtime
                .0
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.queued.retain(|(other, _)| *other != id);
            self.runtime.0.changed.notify_all();
        }
    }
}

/// Exclusive admission. Keep it on the inference worker or child-process owner until
/// all model resources are destroyed. Dropping a UI handle must not release live work.
pub struct Lease {
    runtime: Runtime,
    id: u64,
}
impl Lease {
    /// Publish a lifecycle transition without releasing admission.
    pub fn set_phase(&self, phase: Phase) {
        self.stage(phase, "");
    }
    /// Publish a named stage without releasing its lease.
    pub fn stage(&self, phase: Phase, detail: &str) {
        let mut state = self
            .runtime
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.active.as_ref().map(|(id, _)| *id) == Some(self.id) {
            state.phase = Some(phase);
            state.events.push_back(Transition {
                at: Instant::now(),
                phase,
                detail: detail.into(),
            });
            if state.events.len() > 64 {
                state.events.pop_front();
            }
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let mut state = self
            .runtime
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.active.as_ref().map(|(id, _)| *id) == Some(self.id) {
            state.active = None;
            state.phase = None;
        }
        self.runtime.0.changed.notify_all();
    }
}

#[cfg(test)]
mod tests;
