//! Shared model admission for `MuFoEd`, independent of UI, tensor libraries and model type.
//!
//! Clone one [`Runtime`] into text, vision and image adapters. Hold a [`Lease`] until
//! inference and resource destruction actually finish, even after UI cancellation.
//! Admission reservations are estimates, not measurements of an allocator or free VRAM.
#[cfg(feature = "ollama")]
pub mod ollama;
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
}
impl Task {
    /// Operation with an uncalibrated estimate. Strict budgets will reject it.
    pub fn new(owner: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            model: model.into(),
            device: "auto".into(),
            memory: Memory::default(),
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
    /// Model loading or inference is in progress.
    Running,
    /// Waiting for confirmed model release.
    Releasing,
    /// Release could not be confirmed; the recovery worker retains admission.
    Unknown,
}

#[derive(Default)]
struct State {
    next_id: u64,
    active: Option<(u64, Task)>,
    queued: VecDeque<(u64, Task)>,
    phase: Option<Phase>,
}
struct Inner {
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
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
        }))
    }
    /// Default process-wide instance shared by independently constructed editors/adapters.
    /// Separate OS processes require a broker; this is intentionally process-local.
    pub fn global() -> Self {
        static GLOBAL: OnceLock<Runtime> = OnceLock::new();
        GLOBAL.get_or_init(|| Self::new(Policy::default())).clone()
    }
    /// Validate reservations and enqueue without loading a model or blocking the UI.
    pub fn enqueue(&self, task: Task) -> Result<Ticket, Error> {
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
        if state.queued.len() >= self.0.policy.queue_capacity {
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
}
impl Ticket {
    /// Nonblocking admission. Only the head can start, and only when no lease exists.
    pub fn try_start(&mut self) -> Result<Option<Lease>, Error> {
        let id = self.id.ok_or(Error::AlreadyStarted)?;
        let mut state = self
            .runtime
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.active.is_some() || state.queued.front().map(|(id, _)| *id) != Some(id) {
            return Ok(None);
        }
        state.active = state.queued.pop_front();
        state.phase = Some(Phase::Running);
        self.id = None;
        Ok(Some(Lease {
            runtime: self.runtime.clone(),
            id,
        }))
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
            if state.active.is_none() && state.queued.front().map(|(id, _)| *id) == self.id {
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
        let mut state = self
            .runtime
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.active.as_ref().map(|(id, _)| *id) == Some(self.id) {
            state.phase = Some(phase);
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
