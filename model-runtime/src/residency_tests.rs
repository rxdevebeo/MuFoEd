use super::*;
use crate::{
    telemetry::{Gpu, Observation, Telemetry},
    Policy, Task,
};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
struct Probe {
    free: AtomicU64,
    stale: AtomicBool,
    unknown: AtomicBool,
}
impl Telemetry for Probe {
    fn observe(&self) -> Observation {
        Observation {
            at: Instant::now()
                .checked_sub(if self.stale.load(Ordering::Acquire) {
                    Duration::from_secs(30)
                } else {
                    Duration::ZERO
                })
                .unwrap(),
            ram_total: Some(100_000),
            ram_available: Some(90_000),
            gpus: if self.unknown.load(Ordering::Acquire) {
                vec![]
            } else {
                vec![Gpu {
                    uuid: "GPU-fixed".into(),
                    ordinal: 0,
                    total: 10_000,
                    free: self.free.load(Ordering::Acquire),
                }]
            },
            diagnostic: None,
        }
    }
}
fn setup(free: u64) -> (Runtime, Arc<Probe>) {
    let probe = Arc::new(Probe {
        free: AtomicU64::new(free),
        stale: AtomicBool::new(false),
        unknown: AtomicBool::new(false),
    });
    let runtime = Runtime::managed(
        Policy::default(),
        MemoryPolicy {
            reserve_ram: 0,
            reserve_vram: 0,
            cache_ram: 100_000,
            ..MemoryPolicy::default()
        },
        probe.clone(),
    );
    (runtime, probe)
}
fn acquire(runtime: &Runtime, bytes: u64) -> Result<crate::Lease, crate::Error> {
    let mut task = Task::new("test", "backend");
    task.memory = Memory {
        ram: Some(0),
        vram: Some(bytes),
    };
    runtime
        .enqueue(task)?
        .wait(&AtomicBool::new(false), Duration::from_secs(3))
}
struct Model {
    probe: Arc<Probe>,
    bytes: AtomicU64,
    offload: bool,
    fail: Arc<AtomicBool>,
    unloads: Arc<AtomicUsize>,
}
impl Resource for Model {
    fn memory(&self) -> Memory {
        Memory {
            ram: Some(0),
            vram: Some(self.bytes.load(Ordering::Acquire)),
        }
    }
    fn offload(&self) -> Result<bool, String> {
        if self.offload {
            self.probe
                .free
                .fetch_add(self.bytes.swap(0, Ordering::AcqRel), Ordering::AcqRel);
            Ok(true)
        } else {
            Ok(false)
        }
    }
    fn unload(&self) -> Result<(), String> {
        if self.fail.load(Ordering::Acquire) {
            return Err("unload not confirmed".into());
        }
        self.unloads.fetch_add(1, Ordering::AcqRel);
        self.probe
            .free
            .fetch_add(self.bytes.swap(0, Ordering::AcqRel), Ordering::AcqRel);
        Ok(())
    }
}
fn model(probe: Arc<Probe>, bytes: u64, offload: bool) -> Model {
    Model {
        probe,
        bytes: AtomicU64::new(bytes),
        offload,
        fail: Arc::new(AtomicBool::new(false)),
        unloads: Arc::new(AtomicUsize::new(0)),
    }
}
#[test]
fn insufficient_memory_unknown_and_stale_reject_before_compute() {
    let (r, p) = setup(2000);
    assert!(matches!(
        acquire(&r, 3000),
        Err(crate::Error::OverBudget { .. })
    ));
    assert!(r.snapshot().active.is_none());
    p.unknown.store(true, Ordering::Release);
    assert!(matches!(acquire(&r, 1), Err(crate::Error::Telemetry(_))));
    p.stale.store(true, Ordering::Release);
    assert!(matches!(acquire(&r, 0), Err(crate::Error::Telemetry(_))));
}
#[test]
fn warm_identity_and_active_pins_are_preserved() {
    let (r, p) = setup(9000);
    let lease = acquire(&r, 0).unwrap();
    let loads = AtomicUsize::new(0);
    let first = lease
        .cached("digest-a:vae-x:lora-1", || {
            loads.fetch_add(1, Ordering::Relaxed);
            Ok(model(p.clone(), 1000, false))
        })
        .unwrap();
    lease.release_idle().unwrap();
    assert_eq!(r.residents().len(), 1);
    assert_eq!(r.residents()[0].pins, 1);
    drop(first);
    let second = lease
        .cached::<Model>("digest-a:vae-x:lora-1", || panic!("must reuse"))
        .unwrap();
    assert_eq!(loads.load(Ordering::Relaxed), 1);
    drop(second);
    let changed = lease
        .cached("digest-a:vae-y:lora-1", || Ok(model(p, 1000, false)))
        .unwrap();
    drop(changed);
    assert_eq!(r.residents().len(), 2);
    lease.release_idle().unwrap();
    assert!(r.residents().is_empty());
}
#[test]
fn idle_gpu_offload_is_confirmed_and_remeasured_before_next_allocation() {
    let (r, p) = setup(9000);
    let lease = acquire(&r, 0).unwrap();
    let cached = lease
        .cached("gpu-model", || Ok(model(p.clone(), 4000, true)))
        .unwrap();
    p.free.store(2000, Ordering::Release);
    drop(cached);
    drop(lease);
    let next = acquire(&r, 3500).unwrap();
    assert!(p.free.load(Ordering::Acquire) >= 6000);
    assert_eq!(r.residents()[0].memory.vram, Some(0));
    drop(next);
}
#[test]
fn failed_unload_quarantines_later_admission_until_confirmation() {
    let (r, p) = setup(9000);
    let lease = acquire(&r, 0).unwrap();
    let cached = lease
        .cached("stuck", || Ok(model(p.clone(), 4000, false)))
        .unwrap();
    let fail = cached.fail.clone();
    fail.store(true, Ordering::Release);
    drop(cached);
    assert!(lease.release_idle().is_err());
    drop(lease);
    assert!(matches!(acquire(&r, 0), Err(crate::Error::Backend(_))));
    assert_eq!(r.residents().len(), 1);
    fail.store(false, Ordering::Release);
    let next = acquire(&r, 0).unwrap();
    assert!(r.residents().is_empty());
    drop(next);
}
#[test]
fn oom_retries_once_same_seed_and_other_errors_never_retry() {
    let (r, _) = setup(9000);
    let lease = acquire(&r, 0).unwrap();
    let mut seeds = vec![];
    let value = lease
        .retry(
            Memory {
                ram: Some(0),
                vram: Some(0),
            },
            "cpu",
            &AtomicBool::new(false),
            |attempt| {
                seeds.push(42_u64);
                if attempt == 0 {
                    Err("CUDA out of memory".into())
                } else {
                    Ok(17)
                }
            },
        )
        .unwrap();
    assert_eq!(value, 17);
    assert_eq!(seeds, vec![42, 42]);
    let mut calls = 0;
    let result: Result<(), _> = lease.retry(
        Memory {
            ram: Some(0),
            vram: Some(0),
        },
        "cpu",
        &AtomicBool::new(false),
        |_| {
            calls += 1;
            Err("bad model format".into())
        },
    );
    assert!(result.is_err());
    assert_eq!(calls, 1);
    let mut calls = 0;
    let result: Result<(), _> = lease.retry(
        Memory {
            ram: Some(0),
            vram: Some(0),
        },
        "cpu",
        &AtomicBool::new(false),
        |_| {
            calls += 1;
            Err("CUDA out of memory".into())
        },
    );
    assert!(result.is_err());
    assert_eq!(calls, 2);
}
#[test]
fn failed_loading_never_publishes_a_ready_entry() {
    let (r, _) = setup(9000);
    let lease = acquire(&r, 0).unwrap();
    let result = lease.cached::<Model>("bad", || Err("broken checkpoint".into()));
    assert!(result.is_err());
    assert!(r.residents().is_empty());
}

#[test]
fn queued_warm_reservation_rechecks_after_cache_eviction() {
    let (r, p) = setup(9000);
    let lease = acquire(&r, 0).unwrap();
    let model = lease
        .cached("compatible", || Ok(model(p.clone(), 4000, false)))
        .unwrap();
    drop(model);
    let mut task = Task::new("text", "warm");
    task.memory = Memory {
        ram: Some(0),
        vram: Some(5000),
    };
    task.reuse = vec!["compatible".into()];
    let waiting = r.enqueue(task).unwrap();
    lease.release_idle().unwrap();
    p.free.store(3000, Ordering::Release);
    drop(lease);
    assert!(matches!(
        waiting.wait(&AtomicBool::new(false), Duration::from_secs(2)),
        Err(crate::Error::OverBudget { .. })
    ));
}
#[test]
fn warm_peak_subtracts_owned_residency_only_at_admission() {
    let (r, p) = setup(9000);
    let lease = acquire(&r, 0).unwrap();
    let model = lease
        .cached("compatible", || Ok(model(p.clone(), 4000, false)))
        .unwrap();
    drop(model);
    p.free.store(2000, Ordering::Release);
    drop(lease);
    let mut task = Task::new("text", "warm");
    task.memory = Memory {
        ram: Some(0),
        vram: Some(5000),
    };
    task.reuse = vec!["compatible".into()];
    let lease = r
        .enqueue(task)
        .unwrap()
        .wait(&AtomicBool::new(false), Duration::from_secs(2))
        .unwrap();
    assert_eq!(r.admission().unwrap().estimate.vram, Some(1000));
    drop(lease);
}

#[test]
fn deferred_backend_plan_cannot_bypass_configured_caps() {
    let runtime = Runtime::new(Policy {
        vram: Some(100),
        ..Policy::default()
    });
    let mut task = Task::new("image", "metadata");
    task.memory = Memory {
        ram: Some(0),
        vram: Some(0),
    };
    let lease = runtime.enqueue(task).unwrap().try_start().unwrap().unwrap();
    assert!(matches!(
        lease.prepare(
            Memory {
                ram: Some(0),
                vram: Some(200)
            },
            "auto"
        ),
        Err(crate::Error::OverBudget { .. })
    ));
}
